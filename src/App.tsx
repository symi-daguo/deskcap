import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { DayTimeline24 } from "./DayTimeline24";
import { DayPickerPanel } from "./DayPickerPanel";
import symiMark from "./assets/symi-mark.png";
import deskcapSkillMd from "../skills/deskcap/SKILL.md?raw";
import "./App.css";

type AppContext = { name: string; title: string; path?: string };

type SessionStatus = {
  state: string;
  elapsed: number;
  event_count: number;
  current_app?: AppContext;
  excluded_now: boolean;
  platform: string;
  agent_api: string;
  agent_api_loopback?: string;
  agent_api_lan_ip?: string | null;
  agent_api_bind?: string;
  agent_api_auth?: string;
  local_only?: boolean;
  lan_access?: boolean;
  sessions_bytes: number;
  segment_path?: string;
  accessibility_ok?: boolean;
  input_monitoring_ok?: boolean;
  input_listener_ok?: boolean;
  within_schedule?: boolean;
  schedule_enabled?: boolean;
  last_input_age_secs?: number | null;
  keydown_count?: number;
  mouse_click_count?: number;
  ax_chars?: number;
  exe_path?: string | null;
  from_applications?: boolean;
};

type CaptureConfig = {
  capture_keyboard: boolean;
  capture_mouse: boolean;
  capture_mouse_move: boolean;
  capture_clipboard: boolean;
  exclude_apps: string[];
  screenshot_interval_secs: number;
  segment_duration_secs: number;
  data_dir: string;
  agent_api_port: number;
  agent_api_token: string;
  local_only: boolean;
  retention_days: number;
  max_disk_mb: number;
  backup_enabled: boolean;
  launch_at_login: boolean;
  silent_mode: boolean;
  privacy_consented: boolean;
  schedule_enabled: boolean;
  schedule_start: string;
  schedule_end: string;
  schedule_weekdays: number[];
  update_manifest_url?: string;
};

type UpdateManifest = {
  version: string;
  notes?: string;
  url?: string;
  ime_version?: string;
  ime_url?: string;
};

type CaptureEvent = {
  ts: number;
  wall_ts: number;
  type: string;
  app?: AppContext;
  data: Record<string, unknown>;
};

type MaintenanceReport = {
  deleted_sessions: number;
  deleted_bytes: number;
  sessions_bytes: number;
};

type InstalledApp = { name: string; path: string };
type Nav = "workbench" | "settings" | "agent";

const WEEKDAYS = [
  { id: 1, label: "一" },
  { id: 2, label: "二" },
  { id: 3, label: "三" },
  { id: 4, label: "四" },
  { id: 5, label: "五" },
  { id: 6, label: "六" },
  { id: 7, label: "日" },
];

function formatElapsed(sec: number) {
  const s = Math.floor(sec % 60).toString().padStart(2, "0");
  const m = Math.floor((sec / 60) % 60).toString().padStart(2, "0");
  const h = Math.floor(sec / 3600).toString().padStart(2, "0");
  return `${h}:${m}:${s}`;
}

function formatBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function todayStr() {
  const t = new Date();
  return `${t.getFullYear()}-${(t.getMonth() + 1).toString().padStart(2, "0")}-${t
    .getDate()
    .toString()
    .padStart(2, "0")}`;
}

function compareVer(a: string, b: string) {
  const pa = a.split(".").map((x) => parseInt(x, 10) || 0);
  const pb = b.split(".").map((x) => parseInt(x, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d !== 0) return d > 0 ? 1 : -1;
  }
  return 0;
}

function openExternal(url: string) {
  // Tauri WebView: best-effort open in system browser
  window.open(url, "_blank", "noopener,noreferrer");
}

export default function App() {
  const [nav, setNav] = useState<Nav>("workbench");
  const [status, setStatus] = useState<SessionStatus | null>(null);
  const [themePref, setThemePref] = useState(
    () => localStorage.getItem("deskcap_theme") || "system",
  );
  useEffect(() => {
    const q = window.matchMedia("(prefers-color-scheme: light)");
    const apply = (mode: string) => {
      const light = mode === "light" || (mode === "system" && q.matches);
      document.documentElement.setAttribute("data-theme", light ? "light" : "dark");
    };
    apply(themePref);
    const onChange = () => {
      if ((localStorage.getItem("deskcap_theme") || "system") === "system") apply("system");
    };
    q.addEventListener("change", onChange);
    return () => q.removeEventListener("change", onChange);
  }, [themePref]);
  const [appVersion, setAppVersion] = useState("1.0.19");
  const [config, setConfig] = useState<CaptureConfig | null>(null);
  const [events, setEvents] = useState<CaptureEvent[]>([]);
  const [days, setDays] = useState<string[]>([]);
  // 0.6.56 — 真实存在日账文件的日期集合（日期条上色用）。
  const [daysWithData, setDaysWithData] = useState<Set<string>>(new Set());
  const [selectedDay, setSelectedDay] = useState(todayStr());
  const [dayEvents, setDayEvents] = useState<CaptureEvent[]>([]);
  const [installed, setInstalled] = useState<InstalledApp[]>([]);
  const [appQuery, setAppQuery] = useState("");
  const [excludeText, setExcludeText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showToken, setShowToken] = useState(false);
  const [maintMsg, setMaintMsg] = useState<string | null>(null);
  const [ioMsg, setIoMsg] = useState<string | null>(null);
  const [exportFrom, setExportFrom] = useState("09:00");
  const [exportTo, setExportTo] = useState("18:00");
  const [exportFullDay, setExportFullDay] = useState(false);
  const [skillCopied, setSkillCopied] = useState(false);
  const [skillOpen, setSkillOpen] = useState(false);
  const [autostartOn, setAutostartOn] = useState(false);
  const [updateMsg, setUpdateMsg] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateManifest | null>(null);
  // 0.6.51 — IME 候选译词语言（off = 关闭）。存 IME 自己的 defaults 域。
  const [glossaryLang, setGlossaryLang] = useState<string>("off");
  const [glossaryMsg, setGlossaryMsg] = useState<string | null>(null);
  const [feedPage, setFeedPage] = useState(0);
  const [feedPageSize, setFeedPageSize] = useState(12);
  const feedRef = useRef<HTMLUListElement>(null);
  const workEvents = useMemo(
    () =>
      events.filter((e) => {
        const name = e.app?.name || "";
        const path = e.app?.path || "";
        if (/deskcap/i.test(name) || /DeskCap\.app/i.test(path)) return false;
        // Live stream: typing first; hide raw app_focus noise
        return (
          e.type === "typing" ||
          e.type === "mouse_click" ||
          e.type === "clipboard_paste" ||
          e.type === "clipboard_copy" ||
          e.type === "clipboard_cut"
        );
      }),
    [events],
  );
  const feedNewest = useMemo(
    () => [...workEvents].sort((a, b) => b.wall_ts - a.wall_ts),
    [workEvents],
  );
  useEffect(() => {
    if (nav !== "workbench") return;
    const el = feedRef.current;
    if (!el) return;
    const measure = () => {
      const h = el.clientHeight;
      if (h <= 0) return;
      const row = el.querySelector("li:not(.empty)") as HTMLElement | null;
      const gap = parseFloat(getComputedStyle(el).gap) || 4;
      const rowH = row ? row.offsetHeight + gap : 34;
      const next = Math.max(6, Math.floor(h / rowH));
      setFeedPageSize((p) => (p === next ? p : next));
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  }, [nav, feedNewest.length > 0]);
  const feedPages = Math.max(1, Math.ceil(feedNewest.length / feedPageSize) || 1);
  const feedSafe = Math.min(feedPage, Math.max(0, feedPages - 1));
  const feedSlice = feedNewest.slice(
    feedSafe * feedPageSize,
    feedSafe * feedPageSize + feedPageSize,
  );
  const dayWorkEvents = useMemo(
    () =>
      dayEvents.filter((e) => {
        const name = e.app?.name || "";
        const path = e.app?.path || "";
        return !/deskcap/i.test(name) && !/DeskCap\.app/i.test(path);
      }),
    [dayEvents],
  );

  const refreshDays = useCallback(async () => {
    try {
      // 0.6.56 — 结构化接口：days（含保底今天）+ with_data（真实有日账的日期），
      // 前端给「有数据」的日期实色、「无数据」虚化，并支持跳任意日期。
      const meta = await invoke<{ days: string[]; with_data: string[] }>("list_days_meta");
      const list = meta.days || [];
      setDays(list);
      setDaysWithData(new Set(meta.with_data || []));
      const today = todayStr();
      setSelectedDay((prev) => prev || (list.includes(today) ? today : list[0] || today));
    } catch {
      /* */
    }
  }, []);

  const refreshDayEvents = useCallback(async (day: string) => {
    if (!day) return;
    try {
      const items = await invoke<CaptureEvent[]>("knowledge_day", {
        date: day,
        limit: 2000,
      });
      setDayEvents((prev) => {
        if (
          prev.length === items.length &&
          prev[0]?.wall_ts === items[0]?.wall_ts &&
          prev[prev.length - 1]?.wall_ts === items[items.length - 1]?.wall_ts &&
          prev[prev.length - 1]?.type === items[items.length - 1]?.type
        ) {
          return prev;
        }
        return items;
      });
    } catch {
      /* keep previous day events — never flash empty on transient errors */
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      const st = await invoke<SessionStatus>("get_status");
      setStatus(st);
      if (st.state === "running") {
        const ev = await invoke<CaptureEvent[]>("recent_events", { limit: 30 });
        setEvents((prev) => {
          if (
            prev.length === ev.length &&
            prev[prev.length - 1]?.wall_ts === ev[ev.length - 1]?.wall_ts &&
            prev[prev.length - 1]?.type === ev[ev.length - 1]?.type
          ) {
            return prev;
          }
          return ev;
        });
      }
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadConfig = useCallback(async () => {
    try {
      const cfg = await invoke<CaptureConfig>("get_config");
      setConfig(cfg);
      setExcludeText(cfg.exclude_apps.join("\n"));
      try {
        setAutostartOn(await isEnabled());
      } catch {
        setAutostartOn(cfg.launch_at_login);
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    loadConfig();
    refresh();
    refreshDays();
    getVersion().then(setAppVersion).catch(() => {});
    invoke<InstalledApp[]>("list_installed_apps").then(setInstalled).catch(() => setInstalled([]));
    invoke("mark_ui_ready").catch(() => {});
    // 0.6.51 — 读回 IME 译词语言（macOS only；读失败按 off 显示，不阻塞加载）。
    invoke<string>("get_ime_glossary_language")
      .then((l) => setGlossaryLang(l || "off"))
      .catch(() => setGlossaryLang("off"));
    const statusId = window.setInterval(() => refresh(), 2500);
    return () => window.clearInterval(statusId);
  }, [loadConfig, refresh, refreshDays]);

  useEffect(() => {
    refreshDays();
  }, [refreshDays]);

  useEffect(() => {
    refreshDayEvents(selectedDay);
    // Day ledger slow poll — prevents vis-timeline clear/add flicker.
    const dayId = window.setInterval(() => refreshDayEvents(selectedDay), 12000);
    return () => window.clearInterval(dayId);
  }, [selectedDay, refreshDayEvents]);

  // 0.6.53 修（用户反馈：「DeskCap 最新的日期没有更新到今天」）——
  // `refreshDays` 原来只在 App 挂载时调一次。DeskCap 是 tray 常驻应用，
  // 跨天不重启时日期条永远停在启动那天，今天的新日账不会出现在列表里。
  // 加 60s 低频轮询（日账文件按天创建，1 分钟粒度足够，不打扰）。
  useEffect(() => {
    const daysId = window.setInterval(() => refreshDays(), 60_000);
    return () => window.clearInterval(daysId);
  }, [refreshDays]);

  const filteredApps = useMemo(() => {
    const q = appQuery.trim().toLowerCase();
    const base = q
      ? installed.filter(
          (a) => a.name.toLowerCase().includes(q) || a.path.toLowerCase().includes(q),
        )
      : installed;
    return base.slice(0, 80);
  }, [installed, appQuery]);

  const isMac = status?.platform === "macos";
  const keyboardOk =
    (status?.keydown_count || 0) > 0 || (status?.ax_chars || 0) > 0;
  const monitorOk = status?.input_monitoring_ok === true;
  const axOk = status?.accessibility_ok === true;
  const permsOk = status?.input_listener_ok === true;
  const fromApps = status?.from_applications === true;
  const wrongInstall = isMac && status?.from_applications === false;
  const mouseOnlyTrap =
    isMac && (status?.mouse_click_count || 0) > 0 && !keyboardOk && !permsOk;

  async function saveConfig(next: CaptureConfig) {
    setBusy(true);
    try {
      const saved = await invoke<CaptureConfig>("set_config", { config: next });
      setConfig(saved);
      setExcludeText(saved.exclude_apps.join("\n"));
      try {
        if (saved.launch_at_login) {
          await enable();
          setAutostartOn(true);
        } else {
          await disable();
          setAutostartOn(false);
        }
      } catch (e) {
        setError(`自启动失败: ${e}`);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function toggleSession() {
    if (!config?.privacy_consented && status?.state !== "running") {
      setError("请先在设置中确认隐私说明。");
      setNav("settings");
      return;
    }
    if (isMac && status?.input_listener_ok === false) {
      setError("键鼠监听未就绪：请同时开启「输入监控」和「辅助功能」后等待几秒或重启 DeskCap。");
      setNav("settings");
      return;
    }
    setBusy(true);
    try {
      if (status?.state === "running") await invoke("stop_capture");
      else await invoke("start_capture");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  if (!status || !config) {
    return (
      <div className="boot">
        <div className="boot-card">
          <span className="mark">
            <img src={symiMark} alt="SYMI" onError={(e) => { e.currentTarget.style.display = "none"; }} />
          </span>
          <p>{error ? `启动失败：${error}` : "连接 DeskCap…"}</p>
        </div>
      </div>
    );
  }

  const running = status.state === "running";
  const schedulePaused = status.schedule_enabled && status.within_schedule === false;

  return (
    <div className="shell">
      <header className="topbar">
        <h1>
          <span className="mark sm">
            <img src={symiMark} alt="" />
          </span>
          DeskCap 工作台
        </h1>
        <span className="svc">
          <span className={running ? "dot on" : "dot"} />
          {running ? "采集中" : "空闲"}
          <em>
            · {formatElapsed(status.elapsed)} · 事件 {status.event_count} ·{" "}
            {formatBytes(status.sessions_bytes || 0)}
          </em>
        </span>
        <div className="spacer" />
        <button
          type="button"
          className="ctl"
          title="界面主题：跟随系统 / 浅色 / 深色"
          onClick={() => {
            const next =
              themePref === "system" ? "light" : themePref === "light" ? "dark" : "system";
            localStorage.setItem("deskcap_theme", next);
            setThemePref(next);
          }}
        >
          {themePref === "light" ? "浅色" : themePref === "dark" ? "深色" : "跟随系统"}
        </button>
        <button type="button" className="ctl" title="重新读取状态与日账" onClick={() => { void refresh(); void refreshDays(); void refreshDayEvents(selectedDay); }}>
          刷新
        </button>
        <button
          type="button"
          className={running ? "ctl danger" : "ctl primary"}
          disabled={busy}
          onClick={toggleSession}
        >
          {running ? "停止" : "开始采集"}
        </button>
      </header>

      <nav className="tabs">
        {(
          [
            ["workbench", "工作台"],
            ["settings", "设置"],
            ["agent", "Agent"],
          ] as [Nav, string][]
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            className={nav === id ? "on" : undefined}
            onClick={() => setNav(id)}
          >
            {label}
          </button>
        ))}
      </nav>

      <main className="stage">
        {!config.privacy_consented ? (
          <label className="consent">
            <input
              type="checkbox"
              onChange={(e) => {
                if (e.target.checked) saveConfig({ ...config, privacy_consented: true });
              }}
            />
            <span>
              首次确认：办公打字 / 点击 / 剪贴板纯文本必要采集；支付/密码跳过；本机存储；Agent 只读。确认后不再显示。
            </span>
          </label>
        ) : null}

        {nav === "workbench" && (
          <div className="wb">
            <section className="wb-side">
              <div className="block">
                <div className="block-h">
                  <h2>前台</h2>
                  {wrongInstall ? <span className="warn-t">非 Applications 启动</span> : null}
                  {isMac && !wrongInstall && !permsOk ? <span className="warn-t">权限未齐</span> : null}
                  {mouseOnlyTrap ? <span className="warn-t">仅鼠标·无键盘</span> : null}
                </div>
                {wrongInstall ? (
                  <p className="warn-banner">
                    当前不是从 <code>/Applications/DeskCap.app</code>{" "}
                    启动（构建目录副本无 TCC）。请退出后执行{" "}
                    <code>open -a DeskCap</code>
                    。采集数据在「Application Support/DeskCap」不会因重装丢失。
                  </p>
                ) : isMac && !permsOk ? (
                  <p className="warn-banner">
                    EventTap 未就绪：系统设置里若开关已开仍红灯，多半是<strong>旧 ad-hoc
                    幽灵授权</strong>（看起来已勾、运行时不认）。请删掉所有 DeskCap
                    条目后重新添加 <code>/Applications/DeskCap.app</code>
                    ，或在终端执行{" "}
                    <code>bash scripts/fix-macos-tcc.sh</code>
                    。以后请用{" "}
                    <code>bash scripts/install-macos.sh</code>{" "}
                    覆盖安装（同 Team 签名，一般不用再勾）。
                  </p>
                ) : !keyboardOk ? (
                  <p className="warn-banner soft">
                    权限已就绪，尚未采到打字：请到 Cursor / 微信等窗口打几个字（DeskCap
                    本窗不计）。
                  </p>
                ) : null}
                <p className="app-name">
                  {status.current_app?.name || "—"}
                  {status.excluded_now ? (
                    <i title="当前前台在排除名单：DeskCap 自身（应用名/路径）/ 支付银行 / 密码窗。看本窗时跳过是正常的。工程文件夹名含 deskcap 不会再误伤 Cursor。">
                      跳过
                    </i>
                  ) : null}
                  {schedulePaused ? <i>时段外</i> : null}
                </p>
                <p className="sub">
                  {status.excluded_now
                    ? status.current_app?.name?.toLowerCase() === "deskcap"
                      ? "DeskCap 本窗不采集（请切到记事本 / 微信测试）"
                      : status.current_app?.title || "当前应用已排除"
                    : status.current_app?.title || "等待焦点"}
                </p>
                <dl className="kv">
                  {isMac ? (
                    <>
                      <div>
                        <dt>输入监控</dt>
                        <dd className={monitorOk ? "ok" : "bad"}>
                          {monitorOk ? "已生效" : "未生效"}
                        </dd>
                      </div>
                      <div>
                        <dt>辅助功能</dt>
                        <dd className={axOk ? "ok" : "bad"}>{axOk ? "已生效" : "未生效"}</dd>
                      </div>
                      <div>
                        <dt>EventTap</dt>
                        <dd className={status.input_listener_ok ? "ok" : "bad"}>
                          {status.input_listener_ok ? "已启用" : "未启用"}
                        </dd>
                      </div>
                    </>
                  ) : null}
                  <div>
                    <dt>键盘</dt>
                    <dd className={keyboardOk ? "ok" : "bad"}>
                      {(status.keydown_count || 0) > 0
                        ? `Tap ${status.keydown_count}`
                        : (status.ax_chars || 0) > 0
                          ? `AX ${status.ax_chars}`
                          : "未通"}
                    </dd>
                  </div>
                  <div>
                    <dt>点击</dt>
                    <dd className={(status.mouse_click_count || 0) > 0 ? "ok" : "bad"}>
                      {(status.mouse_click_count || 0) > 0
                        ? `${status.mouse_click_count}`
                        : "尚无"}
                    </dd>
                  </div>
                  <div>
                    <dt>最近键鼠</dt>
                    <dd
                      className={
                        status.last_input_age_secs != null && status.last_input_age_secs < 30
                          ? "ok"
                          : "bad"
                      }
                    >
                      {status.last_input_age_secs == null
                        ? "尚无"
                        : status.last_input_age_secs < 30
                          ? "有信号"
                          : `${Math.round(status.last_input_age_secs)}s前`}
                    </dd>
                  </div>
                </dl>
                {isMac && (status.keydown_count || 0) === 0 && (status.ax_chars || 0) === 0 ? (
                  <button
                    type="button"
                    className="btn ghost"
                    onClick={() => {
                      setNav("settings");
                      invoke("open_accessibility_settings");
                    }}
                  >
                    键盘未通：打开输入监控 + 辅助功能
                  </button>
                ) : null}
              </div>
              <div className="block grow">
                <div className="block-h">
                  <h2>实时流</h2>
                  <span className="muted tiny">
                    工作应用 {workEvents.length} 条 · 每页 {feedPageSize}
                  </span>
                </div>
                <ul className="feed" ref={feedRef}>
                  {feedSlice.length === 0 ? (
                    <li className="empty">开始采集后显示最近事件</li>
                  ) : (
                    feedSlice.map((ev, i) => (
                      <li key={`${ev.wall_ts}-${feedSafe}-${i}`}>
                        <code>{ev.type}</code>
                        <span>
                          {ev.app?.name ? `${ev.app.name} · ` : ""}
                          {ev.type === "typing"
                            ? String(ev.data.text || "").slice(0, 80)
                            : ev.type === "mouse_click"
                              ? `${ev.data.button}`
                              : ev.type.startsWith("clipboard_")
                                ? `${ev.data.action || ""} · ${String(ev.data.text || "").slice(0, 60)}`
                                : ""}
                        </span>
                      </li>
                    ))
                  )}
                </ul>
                {workEvents.length > feedPageSize ? (
                  <div className="feed-pager">
                    <button
                      type="button"
                      className="btn ghost"
                      disabled={feedSafe <= 0}
                      onClick={() => setFeedPage(0)}
                    >
                      最新
                    </button>
                    <span>
                      {feedSafe + 1}/{feedPages}
                    </span>
                    <button
                      type="button"
                      className="btn ghost"
                      disabled={feedSafe >= feedPages - 1}
                      onClick={() => setFeedPage((p) => Math.min(feedPages - 1, p + 1))}
                    >
                      更早
                    </button>
                  </div>
                ) : null}
              </div>
            </section>

            <section className="wb-main">
              <div className="block flat">
                <div className="days">
                  {/* 0.6.57 — 成熟日期面板（react-day-picker，非自造轮子）：
                      带绿点的日期有采集数据，点击即看该天。 */}
                  <DayPickerPanel
                    value={selectedDay}
                    withData={daysWithData}
                    onPick={(v) => {
                      setSelectedDay(v);
                      setDays((prev) => (prev.includes(v) ? prev : [v, ...prev]));
                    }}
                  />
                  {(days.length ? days : [selectedDay]).map((d) => {
                    const hasData = daysWithData.has(d);
                    const cls = selectedDay === d
                      ? "day on"
                      : hasData ? "day has-data" : "day nodata";
                    return (
                      <button
                        key={d}
                        type="button"
                        className={cls}
                        onClick={() => setSelectedDay(d)}
                        title={hasData ? `${d} · 有记录` : `${d} · 暂无记录`}
                      >
                        {d}
                      </button>
                    );
                  })}
                  <div className="spacer" />
                  <div
                    className="export-range"
                    title="导出所选日期的时间切片，写入本机 exports/，不改写日账"
                  >
                    <span className="export-label">
                      时段
                      <em>{selectedDay}</em>
                    </span>
                    <label className="sw compact">
                      <input
                        type="checkbox"
                        checked={exportFullDay}
                        onChange={(e) => setExportFullDay(e.target.checked)}
                      />
                      全天
                    </label>
                    {!exportFullDay ? (
                      <>
                        <input
                          type="time"
                          value={exportFrom}
                          onChange={(e) => setExportFrom(e.target.value || "09:00")}
                          aria-label="导出开始（本地时）"
                        />
                        <span className="muted">–</span>
                        <input
                          type="time"
                          value={exportTo}
                          onChange={(e) => setExportTo(e.target.value || "18:00")}
                          aria-label="导出结束（本地时，不含）"
                        />
                      </>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    className="btn ghost"
                    disabled={busy}
                    onClick={async () => {
                      try {
                        const path = await invoke<string>("export_day", {
                          day: selectedDay,
                          startHhmm: exportFullDay ? null : exportFrom,
                          endHhmm: exportFullDay ? null : exportTo,
                        });
                        setIoMsg(
                          exportFullDay
                            ? `已导出 ${selectedDay} 全天 → ${path}`
                            : `已导出 ${selectedDay} ${exportFrom}–${exportTo} → ${path}`,
                        );
                      } catch (e) {
                        setError(String(e));
                      }
                    }}
                  >
                    导出样本
                  </button>
                </div>
                {ioMsg ? <p className="ok-line">{ioMsg}</p> : null}
                <DayTimeline24
                  date={selectedDay}
                  events={dayWorkEvents}
                  onStaleDay={() => setSelectedDay(todayStr())}
                />
              </div>
            </section>
          </div>
        )}

        {nav === "settings" && (
          <div className="settings">
            {isMac ? (
            <div className="banner">
              <div className="banner-head">
                <strong>权限与安装</strong>
                <button
                  type="button"
                  className="btn ghost"
                  onClick={() => invoke("open_accessibility_settings")}
                >
                  打开系统设置
                </button>
              </div>
              <p className="kv-inline">
                路径：
                <b className={fromApps && !wrongInstall ? "ok" : "bad"}>
                  {wrongInstall ? "非 Applications" : "Applications"}
                </b>
                {" · "}
                EventTap：
                <b className={status.input_listener_ok ? "ok" : "bad"}>
                  {status.input_listener_ok ? "就绪" : "未就绪"}
                </b>
                {" · "}
                键盘：
                <b className={keyboardOk ? "ok" : "bad"}>
                  {(status.keydown_count || 0) > 0
                    ? `Tap ${status.keydown_count}`
                    : (status.ax_chars || 0) > 0
                      ? `AX ${status.ax_chars}`
                      : "未通"}
                </b>
                {" · "}
                点击：
                <b className={(status.mouse_click_count || 0) > 0 ? "ok" : "bad"}>
                  {status.mouse_click_count || 0}
                </b>
              </p>
              {wrongInstall ? (
                <p className="warn-banner">
                  错误路径：{status.exe_path || "未知"}。请用 <code>open -a DeskCap</code>。
                </p>
              ) : (
                <p className="tiny muted">
                  仅安装 <code>/Applications/DeskCap.app</code>
                  ；关闭窗口会回菜单栏，不退出。
                </p>
              )}
            </div>
            ) : null}

            <div className="settings-stack">
              <section className="block">
                <h2>{isMac ? "菜单栏与启动" : "托盘与启动"}</h2>
                <p className="sub">
                  {isMac
                    ? "菜单栏右上角常驻；登录后默认可无窗后台采集。"
                    : "任务栏右下角托盘常驻；登录后默认可无窗后台采集。"}
                </p>
                <div className="sw-grid">
                  <label className="sw">
                    <input
                      type="checkbox"
                      checked={config.launch_at_login || autostartOn}
                      onChange={(e) =>
                        saveConfig({ ...config, launch_at_login: e.target.checked })
                      }
                    />
                    登录时打开
                  </label>
                  <label className="sw">
                    <input
                      type="checkbox"
                      checked={config.silent_mode}
                      onChange={(e) =>
                        saveConfig({ ...config, silent_mode: e.target.checked })
                      }
                    />
                    {isMac ? "登录后仅菜单栏（不弹窗）" : "登录后仅托盘（不弹窗）"}
                  </label>
                </div>
              </section>

              <section className="block">
                <h2>采集内容</h2>
                <div className="sw-grid">
                  <label className="sw">
                    <input
                      type="checkbox"
                      checked={config.capture_keyboard}
                      onChange={(e) =>
                        saveConfig({ ...config, capture_keyboard: e.target.checked })
                      }
                    />
                    键盘聚合
                  </label>
                  <label className="sw">
                    <input
                      type="checkbox"
                      checked={config.capture_mouse}
                      onChange={(e) =>
                        saveConfig({ ...config, capture_mouse: e.target.checked })
                      }
                    />
                    鼠标点击
                  </label>
                  <label className="sw">
                    <input
                      type="checkbox"
                      checked={config.capture_clipboard !== false}
                      onChange={(e) =>
                        saveConfig({ ...config, capture_clipboard: e.target.checked })
                      }
                    />
                    剪贴板纯文本（{isMac ? "⌘C/V/X" : "Ctrl+C/V/X"}）
                  </label>
                </div>
              </section>

              <div className="settings-row2">
                <section className="block">
                  <h2>采集时段</h2>
                  <p className="sub">关闭 = 全天 24h</p>
                  <label className="sw">
                    <input
                      type="checkbox"
                      checked={config.schedule_enabled}
                      onChange={(e) =>
                        saveConfig({ ...config, schedule_enabled: e.target.checked })
                      }
                    />
                    启用时段覆盖
                  </label>
                  <div className="row2">
                    <label>
                      开始
                      <input
                        type="time"
                        value={config.schedule_start}
                        onChange={(e) =>
                          saveConfig({
                            ...config,
                            schedule_start: e.target.value || "09:00",
                          })
                        }
                      />
                    </label>
                    <label>
                      结束
                      <input
                        type="time"
                        value={config.schedule_end}
                        onChange={(e) =>
                          saveConfig({
                            ...config,
                            schedule_end: e.target.value || "18:30",
                          })
                        }
                      />
                    </label>
                  </div>
                  <div className="wds">
                    {WEEKDAYS.map((d) => {
                      const on = config.schedule_weekdays.includes(d.id);
                      return (
                        <button
                          key={d.id}
                          type="button"
                          className={on ? "wd on" : "wd"}
                          onClick={() => {
                            const set = new Set(config.schedule_weekdays);
                            if (on) set.delete(d.id);
                            else set.add(d.id);
                            saveConfig({
                              ...config,
                              schedule_weekdays: Array.from(set).sort(),
                            });
                          }}
                        >
                          {d.label}
                        </button>
                      );
                    })}
                  </div>
                </section>

                <section className="block">
                  <h2>存储</h2>
                  <div className="row2">
                    <label className="field">
                      截图秒（0=关）
                      <input
                        type="number"
                        min={0}
                        value={config.screenshot_interval_secs}
                        onChange={(e) =>
                          saveConfig({
                            ...config,
                            screenshot_interval_secs: Number(e.target.value) || 0,
                          })
                        }
                      />
                    </label>
                    <label className="field">
                      保留天
                      <input
                        type="number"
                        min={1}
                        value={config.retention_days}
                        onChange={(e) =>
                          saveConfig({
                            ...config,
                            retention_days: Math.max(1, Number(e.target.value) || 14),
                          })
                        }
                      />
                    </label>
                    <label className="field">
                      上限 MB
                      <input
                        type="number"
                        min={100}
                        value={config.max_disk_mb}
                        onChange={(e) =>
                          saveConfig({
                            ...config,
                            max_disk_mb: Math.max(100, Number(e.target.value) || 1024),
                          })
                        }
                      />
                    </label>
                  </div>
                  <button
                    type="button"
                    className="btn ghost"
                    disabled={busy}
                    onClick={async () => {
                      const r = await invoke<MaintenanceReport>("run_maintenance");
                      setMaintMsg(
                        `清理 ${r.deleted_sessions}（${formatBytes(r.deleted_bytes)}）占用 ${formatBytes(r.sessions_bytes)}`,
                      );
                      refresh();
                    }}
                  >
                    清理 / 备份
                  </button>
                  {maintMsg ? <p className="ok-line">{maintMsg}</p> : null}
                </section>

                <section className="block">
                  <h2>检查更新（OTA）</h2>
                  <p className="tiny muted">
                    填写公司更新源 HTTPS 地址（返回 JSON：version / notes / url /
                    ime_version）。空 = 关闭。不会上传任何采集数据。
                  </p>
                  <label className="field">
                    更新源 URL
                    <input
                      placeholder="https://…/deskcap-manifest.json"
                      value={config.update_manifest_url || ""}
                      onChange={(e) =>
                        saveConfig({ ...config, update_manifest_url: e.target.value.trim() })
                      }
                    />
                  </label>
                  <div className="row">
                    <button
                      type="button"
                      className="btn"
                      disabled={busy}
                      onClick={async () => {
                        setUpdateMsg(null);
                        setUpdateInfo(null);
                        const url = (config.update_manifest_url || "").trim();
                        if (!url) {
                          setUpdateMsg("未配置更新源，已跳过。");
                          return;
                        }
                        try {
                          const res = await fetch(url, { cache: "no-store" });
                          if (!res.ok) throw new Error(`HTTP ${res.status}`);
                          const man = (await res.json()) as UpdateManifest;
                          if (!man?.version) throw new Error("manifest 缺少 version");
                          const newer = compareVer(man.version, appVersion) > 0;
                          setUpdateInfo(man);
                          setUpdateMsg(
                            newer
                              ? `发现新版本 ${man.version}（当前 ${appVersion}）${man.notes ? " — " + man.notes : ""}`
                              : `已是最新（${appVersion}）· 源版本 ${man.version}`,
                          );
                        } catch (e) {
                          setUpdateMsg(`检查失败：${e instanceof Error ? e.message : String(e)}`);
                        }
                      }}
                    >
                      检查更新
                    </button>
                    {updateInfo?.url && updateMsg?.startsWith("发现新版本") ? (
                      <button
                        type="button"
                        className="btn ghost"
                        onClick={() => {
                          if (updateInfo?.url) openExternal(updateInfo.url);
                        }}
                      >
                        打开下载页
                      </button>
                    ) : null}
                  </div>
                  {updateMsg ? <p className="ok-line">{updateMsg}</p> : null}
                  {updateInfo?.ime_version ? (
                    <p className="tiny muted">
                      源内输入法版本：{updateInfo.ime_version}
                      {updateInfo.ime_url ? (
                        <>
                          {" · "}
                          <button
                            type="button"
                            className="btn ghost"
                            onClick={() => openExternal(updateInfo.ime_url!)}
                          >
                            打开 IME 下载
                          </button>
                        </>
                      ) : null}
                    </p>
                  ) : null}
                </section>
              </div>

              {/* 0.6.51 — IME 候选译词设置。之前只能命令行 defaults write，
                  用户要求「设置输入法的界面也要匹配正确」。 */}
              <section className="block">
                <h2>输入法 · 候选译词</h2>
                <p className="tiny muted">
                  在候选词下方显示英文释义，边打字边学英语。数据为内置词表
                  （240 单字 + 166 词组），用户目录词表优先，纯本地不联网。
                </p>
                <div className="row" style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <label htmlFor="glossary-lang">译词语言</label>
                  <select
                    id="glossary-lang"
                    value={glossaryLang}
                    onChange={(e) => {
                      const lang = e.target.value;
                      setGlossaryLang(lang);
                      invoke("set_ime_glossary_language", { lang })
                        .then(() =>
                          setGlossaryMsg(
                            lang === "off" ? "已关闭译词，下次出候选生效" : `已设为 ${lang}，下次出候选生效`
                          )
                        )
                        .catch((err) => setGlossaryMsg(`设置失败：${String(err)}`));
                    }}
                  >
                    <option value="off">关闭</option>
                    <option value="en">英语 English</option>
                  </select>
                  {glossaryMsg ? <span className="tiny muted">{glossaryMsg}</span> : null}
                </div>
                <p className="tiny muted">
                  仅 macOS 生效。切换输入法到「DeskCap拼音」后打拼音即可看到释义；
                  回车上屏原始字母（输英文/网址），空格上屏第一候选。
                </p>
              </section>

              <section className="block">
                <h2>排除应用</h2>
                <div className="exclude">
                  <div>
                    <input
                      className="search"
                      placeholder="搜索本机应用"
                      value={appQuery}
                      onChange={(e) => setAppQuery(e.target.value)}
                    />
                    <ul className="apps">
                      {filteredApps.map((a) => {
                        const on = config.exclude_apps.includes(a.name);
                        return (
                          <li key={a.path}>
                            <button
                              type="button"
                              className={on ? "pick on" : "pick"}
                              onClick={() => {
                                const set = new Set(config.exclude_apps);
                                if (on) set.delete(a.name);
                                else set.add(a.name);
                                saveConfig({
                                  ...config,
                                  exclude_apps: Array.from(set),
                                });
                              }}
                            >
                              {a.name}
                              <em>{on ? "排除" : "+"}</em>
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  </div>
                  <textarea
                    value={excludeText}
                    onChange={(e) => setExcludeText(e.target.value)}
                    onBlur={() =>
                      saveConfig({
                        ...config,
                        exclude_apps: excludeText
                          .split("\n")
                          .map((s) => s.trim())
                          .filter(Boolean),
                      })
                    }
                    rows={10}
                  />
                </div>
              </section>
            </div>
          </div>
        )}

        {nav === "agent" && (
          <div className="agent-page">
            <div className="block">
              <h2>引导智能体 · 内置 Skill</h2>
              <p className="sub">
                Skill 已内嵌（无需单独 md 文件）。默认折叠；展开可复制。
                <b>点击次数只用 summary</b>，不要拉「微信 · left」这类逐条 click。
              </p>
              <div className="row">
                <button
                  type="button"
                  className="btn ghost"
                  onClick={() => setSkillOpen((v) => !v)}
                >
                  {skillOpen ? "收起 Skill" : "展开 Skill"}
                </button>
                <button
                  type="button"
                  className="btn primary"
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(deskcapSkillMd);
                      setSkillCopied(true);
                      window.setTimeout(() => setSkillCopied(false), 2000);
                    } catch (e) {
                      setError(`复制失败: ${e}`);
                    }
                  }}
                >
                  {skillCopied ? "已复制 Skill" : "复制 Skill 全文"}
                </button>
                <button type="button" className="btn ghost" onClick={() => setShowToken((v) => !v)}>
                  {showToken ? "隐藏 Token" : "显示 Token"}
                </button>
                <button
                  type="button"
                  className="btn ghost"
                  disabled={busy}
                  onClick={async () => {
                    setConfig(await invoke<CaptureConfig>("rotate_api_token"));
                    setShowToken(true);
                  }}
                >
                  轮换 Token
                </button>
              </div>
              <p>
                局域网（给 Hermes bot） <code>{status.agent_api}</code>
                {status.agent_api_loopback ? (
                  <>
                    {" "}
                    · 本机 <code>{status.agent_api_loopback}</code>
                  </>
                ) : null}{" "}
                · {appVersion} · {status.platform}
              </p>
              {showToken ? <pre className="token">{config.agent_api_token}</pre> : null}
              <p className="tiny muted">
                Token = <code>dc_</code> + UUID v4；只给<strong>自己的</strong> bot。轮换后旧 Token
                立即失效。敏感路由必须 <code>Authorization: Bearer …</code>。
                若 Hermes 连不上：放行入站 TCP {config.agent_api_port || 8765}
                （macOS 防火墙 / Windows 防火墙 / Linux ufw）。
              </p>
              {skillOpen ? <pre className="skill-doc">{deskcapSkillMd}</pre> : null}
            </div>

            <div className="principle-grid">
              <article className="principle">
                <h3>只读契约</h3>
                <ul>
                  <li>Agent 只能 GET，不能改写采集库</li>
                  <li>启停 / 导出样本 / 配置仅 DeskCap 界面（无导入）</li>
                  <li>无云上传；Hermes 用局域网 IP + 每人自管 Token 拉取</li>
                </ul>
              </article>
              <article className="principle">
                <h3>采什么</h3>
                <ul>
                  <li>打字、点击、剪贴板纯文本</li>
                  <li>支付 / 密码窗 / DeskCap 自身跳过</li>
                  <li>点击量只用 summary，禁止采「left/left…」流水</li>
                </ul>
              </article>
              <article className="principle">
                <h3>先汇总后明细</h3>
                <ul>
                  <li>
                    <code>/knowledge/day/summary</code> → 各 App 点击/打字次数
                  </li>
                  <li>
                    需要正文时再 <code>/knowledge/day?since=</code>
                  </li>
                  <li>单轮 30–80 条；禁止整日灌库</li>
                </ul>
              </article>
              <article className="principle">
                <h3>微信 / 企微</h3>
                <ul>
                  <li>常为拼音；Agent 自库翻译</li>
                  <li>粘贴长文看剪贴板事件</li>
                </ul>
              </article>
              <article className="principle">
                <h3>隐私商用</h3>
                <ul>
                  <li>采集侧 [REDACTED]；Agent 仍须再扫</li>
                  <li>拒绝展示 Token / 密码</li>
                  <li>导出仅本机时段切片</li>
                </ul>
              </article>
              <article className="principle">
                <h3>时间轴</h3>
                <ul>
                  <li>拖拽/点空白吸附最近事件竖条</li>
                  <li>上一条 / 下一条可连续点击</li>
                  <li>渲染库 vis-timeline；数据自有</li>
                </ul>
              </article>
            </div>

            <div className="block">
              <h2>推荐同步顺序</h2>
              <ol className="steps">
                <li>
                  <code>GET /health</code>
                </li>
                <li>
                  <code>GET /status</code> → <code>input_listener_ok</code>
                </li>
                <li>
                  <code>GET /knowledge/day/summary?date=今天</code> → 活跃 App / 点击量
                </li>
                <li>
                  需要正文：
                  <code>GET /knowledge/day?date=&amp;limit=80&amp;since=游标</code>
                </li>
                <li>去重 → 隐私 →（微信）拼音翻译 → Agent 自有库 → <code>next_since</code></li>
              </ol>
            </div>
          </div>
        )}

        {error ? <p className="err">{error}</p> : null}
      </main>
    </div>
  );
}
