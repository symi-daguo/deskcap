import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DataSet } from "vis-data";
import { Timeline } from "vis-timeline/standalone";
import "vis-timeline/styles/vis-timeline-graph2d.min.css";
import { addDays, addMinutes, format, startOfDay, parseISO } from "date-fns";

export type TimelineEvent = {
  wall_ts: number;
  type: string;
  app?: { name: string; title?: string };
  data: Record<string, unknown>;
};

type Props = {
  date: string;
  events: TimelineEvent[];
  /** 0.6.52 — 跨天且用户长时间无交互时通知上层把 selectedDay 切回今天。 */
  onStaleDay?: () => void;
};

type Filter = "all" | "typing" | "mouse_click" | "clipboard";

function isClipboard(t: string) {
  return t.startsWith("clipboard_");
}

function summarize(ev: TimelineEvent) {
  if (ev.type === "typing") {
    const t = String(ev.data.text || "");
    return t.length > 140 ? `${t.slice(0, 140)}…` : t;
  }
  if (ev.type === "mouse_click") return `${ev.data.button} @ (${ev.data.x}, ${ev.data.y})`;
  if (isClipboard(ev.type)) {
    const t = String(ev.data.text || "");
    const head = t.length > 140 ? `${t.slice(0, 140)}…` : t;
    return `${ev.data.action || ev.type} · ${head}`;
  }
  return JSON.stringify(ev.data);
}

function toItems(events: TimelineEvent[]) {
  return events.map((ev, i) => {
    const clip = isClipboard(ev.type);
    const startMs =
      ev.type === "typing" && ev.data.start_wall
        ? Number(ev.data.start_wall) * 1000
        : ev.wall_ts * 1000;
    const endMs =
      ev.type === "typing" && ev.data.end_wall
        ? Math.max(Number(ev.data.end_wall) * 1000, startMs + 1000)
        : startMs + (ev.type === "typing" ? 8000 : clip ? 4000 : 2000);
    return {
      id: `${ev.type}-${ev.wall_ts}-${i}`,
      group: ev.type === "typing" ? "typing" : clip ? "clipboard" : "click",
      content: ev.type === "typing"
        ? String(ev.data.text || "").slice(0, 40)
        : clip
          ? `${ev.data.action || "clip"} · ${String(ev.data.text || "").slice(0, 28)}`
          : `${ev.data.button ?? "click"}`,
      start: new Date(startMs),
      end: new Date(endMs),
      title: `${ev.type} · ${ev.app?.name || ""} · ${summarize(ev)}`,
      className:
        ev.type === "typing" ? "item-typing" : clip ? "item-clip" : "item-click",
    };
  });
}

function eventsKey(events: TimelineEvent[]) {
  if (events.length === 0) return "0";
  const first = events[0];
  const last = events[events.length - 1];
  return `${events.length}:${first.wall_ts}:${last.wall_ts}:${last.type}`;
}

function eventId(ev: TimelineEvent, i: number) {
  return `${ev.type}-${ev.wall_ts}-${i}`;
}

/** NVR-style 24h: hour index + search + window jump; list shows focused slice. */
function DayTimeline24Inner({ date, events, onStaleDay }: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const tlRef = useRef<Timeline | null>(null);
  const itemsRef = useRef<DataSet<object> | null>(null);
  const eventsRef = useRef(events);
  eventsRef.current = events;
  const lastKey = useRef("");
  const [focusMin, setFocusMin] = useState(() => {
    const d = new Date();
    return d.getHours() * 60 + d.getMinutes();
  });
  const [filter, setFilter] = useState<Filter>("typing");
  const [q, setQ] = useState("");
  const [jump, setJump] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedEv, setSelectedEv] = useState<TimelineEvent | null>(null);
  const [listPage, setListPage] = useState(0);

  // 0.6.52 修（用户反馈：「工作台默认必须要是当天最新的采集数据，目前显示的不对」）——
  // DeskCap 是 tray 常驻应用，工作台窗口经常一开就是几小时/几天。原实现里
  // focusMin 只在**组件挂载时**快照一次当前时刻（useState 初始化器），之后
  // 24h 视窗与右侧列表就钉死在那个时刻——新采集的事件落在视野外，看起来
  // 「不是最新的」。跨天同理：selectedDay 停在打开那天的日期。
  //
  // 修法（最小改动，不加层）：
  //   1. 每 30s 检查一次——若在看今天、且用户 60s 内没有任何交互，
  //      视窗自动滑到「现在」；跨天且 5 分钟无交互则通知上层切回今天。
  //   2. 用户一旦有交互（拖时间轴/点选/筛选/翻页），60s 内绝不抢焦点，
  //      保证正在浏览历史时不被拽走。
  //   （interval 本体在 moveWindowRef 声明之后，见下。）
  const lastUserActionRef = useRef(Date.now());
  const markUserAction = useCallback(() => {
    lastUserActionRef.current = Date.now();
  }, []);

  const cursorTsRef = useRef<number | null>(null);
  const snapLock = useRef(false);

  const day = useMemo(() => startOfDay(parseISO(date)), [date]);
  const key = eventsKey(events);

  const filtered = useMemo(() => {
    const qq = q.trim().toLowerCase();
    return events.filter((e) => {
      const name = e.app?.name || "";
      if (/deskcap/i.test(name)) return false;
      if (filter === "clipboard") {
        if (!isClipboard(e.type)) return false;
      } else if (filter !== "all" && e.type !== filter) {
        return false;
      }
      if (!qq) return true;
      const hay = `${e.app?.name || ""} ${summarize(e)}`.toLowerCase();
      return hay.includes(qq);
    });
  }, [events, filter, q]);

  const hourDensity = useMemo(() => {
    const bins = Array.from({ length: 24 }, () => 0);
    for (const e of filtered) {
      const h = new Date(e.wall_ts * 1000).getHours();
      if (h >= 0 && h < 24) bins[h] += 1;
    }
    const max = Math.max(1, ...bins);
    return bins.map((n) => n / max);
  }, [filtered]);

  const nearby = useMemo(() => {
    const center = day.getTime() / 1000 + focusMin * 60;
    const lo = center - 30 * 60;
    const hi = center + 30 * 60;
    return filtered
      .filter((e) => e.wall_ts >= lo && e.wall_ts <= hi)
      .sort((a, b) => b.wall_ts - a.wall_ts);
  }, [filtered, day, focusMin]);

  const [pageSize, setPageSize] = useState(8);
  const listRef = useRef<HTMLUListElement>(null);
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const measure = () => {
      const h = el.clientHeight;
      if (h <= 0) return;
      const row = el.querySelector("li:not(.empty)") as HTMLElement | null;
      const gap = parseFloat(getComputedStyle(el).gap) || 6;
      const rowH = row ? row.offsetHeight + gap : 56;
      const next = Math.max(4, Math.floor(h / rowH));
      setPageSize((p) => (p === next ? p : next));
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  }, [nearby.length > 0]);
  const pageCount = Math.max(1, Math.ceil(nearby.length / pageSize));
  const pageSafe = Math.min(listPage, pageCount - 1);
  const pageItems = nearby.slice(pageSafe * pageSize, pageSafe * pageSize + pageSize);

  useEffect(() => {
    setListPage(0);
  }, [focusMin, filter, q, date]);

  const moveWindow = (centerMin: number, spanMin = 120) => {
    const tl = tlRef.current;
    if (!tl) return;
    const c = Math.max(0, Math.min(1439, centerMin));
    const half = spanMin / 2;
    const start = addMinutes(day, Math.max(0, c - half));
    const end = addMinutes(day, Math.min(1440, c + half));
    snapLock.current = true;
    tl.setWindow(start, end, { animation: false });
    setFocusMin(c);
    window.setTimeout(() => {
      snapLock.current = false;
    }, 80);
  };
  // 0.6.52 — 自动跟随 interval 引用 moveWindow（定义在此处之后才能引用，
  // 用 ref 转发；interval 只在用户空闲时才调用，时序安全）。
  const moveWindowRef = useRef<((centerMin: number, spanMin?: number) => void) | null>(null);
  moveWindowRef.current = moveWindow;

  useEffect(() => {
    const id = window.setInterval(() => {
      const idleMs = Date.now() - lastUserActionRef.current;
      const today = new Date();
      const todayStr = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
      // 跨天 + 5 分钟无交互 → 通知上层切回今天（用户不在浏览旧数据时才动）。
      if (idleMs > 5 * 60_000 && date !== todayStr) {
        onStaleDay?.();
        return;
      }
      // 在看今天 + 60s 无交互 → 视窗滑到「现在」，列表跟着出现最新事件。
      if (idleMs > 60_000) {
        const d = parseISO(date);
        const sameDay =
          d.getFullYear() === today.getFullYear() &&
          d.getMonth() === today.getMonth() &&
          d.getDate() === today.getDate();
        if (sameDay) {
          moveWindowRef.current?.(today.getHours() * 60 + today.getMinutes(), 120);
        }
      }
    }, 30_000);
    return () => window.clearInterval(id);
  }, [date, onStaleDay]);

  const snapToNearestEvent = (centerSec: number, spanMin = 60) => {
    const list = eventsRef.current;
    if (list.length === 0) return;
    let best = list[0];
    let bestDist = Math.abs(best.wall_ts - centerSec);
    for (const e of list) {
      const d = Math.abs(e.wall_ts - centerSec);
      if (d < bestDist) {
        best = e;
        bestDist = d;
      }
    }
    if (bestDist > 180) return;
    cursorTsRef.current = best.wall_ts;
    const min = Math.round((best.wall_ts * 1000 - day.getTime()) / 60000);
    moveWindow(min, spanMin);
    const i = list.findIndex(
      (e) => e.wall_ts === best.wall_ts && e.type === best.type,
    );
    const id = eventId(best, i >= 0 ? i : 0);
    setSelectedId(id);
    setSelectedEv(best);
    tlRef.current?.setSelection([id]);
  };
  const snapRef = useRef(snapToNearestEvent);
  snapRef.current = snapToNearestEvent;
  // 0.6.52 — markUserAction 的 ref 转发（TL 事件回调在此处之前注册）。
  const markUserActionRef = useRef<(() => void) | null>(null);
  markUserActionRef.current = markUserAction;

  const jumpToClock = (text: string) => {
    markUserAction();
    const m = text.trim().match(/^(\d{1,2}):(\d{2})$/);
    if (!m) return;
    const hh = Number(m[1]);
    const mm = Number(m[2]);
    if (hh > 23 || mm > 59) return;
    moveWindow(hh * 60 + mm, 90);
  };

  useEffect(() => {
    if (!hostRef.current) return;
    const items = new DataSet(toItems(eventsRef.current));
    itemsRef.current = items as unknown as DataSet<object>;
    lastKey.current = `${eventsKey(eventsRef.current)}|all|`;
    const groups = new DataSet([
      { id: "typing", content: "打字" },
      { id: "clipboard", content: "剪贴板" },
      { id: "click", content: "点击" },
    ]);
    const now = new Date();
    const sameDay =
      now.getFullYear() === day.getFullYear() &&
      now.getMonth() === day.getMonth() &&
      now.getDate() === day.getDate();
    const centerMin = sameDay ? now.getHours() * 60 + now.getMinutes() : 12 * 60;
    const initStart = addMinutes(day, Math.max(0, centerMin - 60));
    const initEnd = addMinutes(day, Math.min(1440, centerMin + 60));

    const tl = new Timeline(hostRef.current, items, groups, {
      width: "100%",
      height: "200px",
      min: day,
      max: addDays(day, 1),
      start: initStart,
      end: initEnd,
      zoomMin: 1000 * 60 * 5,
      zoomMax: 1000 * 60 * 60 * 24,
      stack: true,
      orientation: "top",
      showCurrentTime: sameDay,
      editable: false,
      selectable: true,
      multiselect: false,
      margin: { item: 4, axis: 8 },
    });
    tlRef.current = tl;
    setFocusMin(centerMin);

    const syncFocus = (props?: { byUser?: boolean }) => {
      if (snapLock.current) return;
      const w = tl.getWindow();
      const midMs = (w.start.getTime() + w.end.getTime()) / 2;
      const next = Math.max(0, Math.min(1439, Math.round((midMs - day.getTime()) / 60000)));
      setFocusMin((prev) => (prev === next ? prev : next));
      if (props?.byUser) {
        markUserActionRef.current?.();
        const spanMin = Math.max(
          5,
          Math.round((w.end.getTime() - w.start.getTime()) / 60000),
        );
        snapRef.current(midMs / 1000, Math.min(120, spanMin));
      }
    };
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tl.on("rangechanged", (props: any) => syncFocus(props));
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tl.on("click", (props: any) => {
      const t = props?.time as Date | undefined;
      if (!t || props?.item) return;
      markUserActionRef.current?.();
      snapRef.current(t.getTime() / 1000, 60);
    });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tl.on("select", (props: any) => {
      if (props?.items?.[0]) markUserActionRef.current?.();
      const id = props?.items?.[0];
      if (!id) {
        setSelectedId(null);
        setSelectedEv(null);
        return;
      }
      const sid = String(id);
      setSelectedId(sid);
      const hit = eventsRef.current.find((e, i) => eventId(e, i) === sid);
      if (hit) {
        cursorTsRef.current = hit.wall_ts;
        setSelectedEv(hit);
      } else {
        setSelectedEv(null);
      }
    });
    return () => {
      tl.destroy();
      tlRef.current = null;
      itemsRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- remount only on day change
  }, [date, day]);

  useEffect(() => {
    const ds = itemsRef.current;
    if (!ds) return;
    const stamp = `${eventsKey(filtered)}|${filter}|${q}`;
    if (stamp === lastKey.current) return;
    lastKey.current = stamp;
    const next = toItems(filtered);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (ds as any).clear();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (ds as any).add(next);
  }, [filtered, filter, q, key]);

  const typingCount = useMemo(
    () => filtered.filter((e) => e.type === "typing").length,
    [filtered],
  );
  const clickCount = useMemo(
    () => filtered.filter((e) => e.type === "mouse_click").length,
    [filtered],
  );
  const clipCount = useMemo(
    () => filtered.filter((e) => isClipboard(e.type)).length,
    [filtered],
  );

  const goPrevNext = (dir: -1 | 1) => {
    if (filtered.length === 0) return;
    const sorted = [...filtered].sort((a, b) => a.wall_ts - b.wall_ts);
    const cur =
      cursorTsRef.current ??
      selectedEv?.wall_ts ??
      day.getTime() / 1000 + focusMin * 60;
    let idx: number;
    if (dir === 1) {
      idx = sorted.findIndex((e) => e.wall_ts > cur + 1e-6);
      if (idx < 0) idx = sorted.length - 1;
    } else {
      idx = 0;
      for (let i = sorted.length - 1; i >= 0; i -= 1) {
        if (sorted[i].wall_ts < cur - 1e-6) {
          idx = i;
          break;
        }
      }
    }
    const next = sorted[idx];
    if (!next) return;
    cursorTsRef.current = next.wall_ts;
    const min = Math.round((next.wall_ts * 1000 - day.getTime()) / 60000);
    moveWindow(min, 60);
    const i = events.findIndex(
      (e) => e.wall_ts === next.wall_ts && e.type === next.type,
    );
    const id = eventId(next, i >= 0 ? i : 0);
    setSelectedId(id);
    setSelectedEv(next);
    tlRef.current?.setSelection([id]);
  };

  return (
    <div className="nvr">
      <div className="nvr-meta">
        <div>
          <strong>{date}</strong>
          <span className="muted"> · 拖拽后吸附最近事件；上一条/下一条可连续点</span>
        </div>
        <div className="nvr-cursor-time">
          {format(new Date(day.getTime() + focusMin * 60000), "HH:mm")}
        </div>
      </div>

      <div className="nvr-tools">
        <div className="nvr-filters">
          {(
            [
              ["all", "全部"],
              ["typing", "打字"],
              ["clipboard", "剪贴板"],
              ["mouse_click", "点击"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={filter === id ? "chip on" : "chip"}
              onClick={() => { markUserAction(); setFilter(id); }}
            >
              {label}
            </button>
          ))}
        </div>
        <input
          className="nvr-search"
          placeholder="搜索 app / 文本…"
          value={q}
          onChange={(e) => { markUserAction(); setQ(e.target.value); }}
        />
        <form
          className="nvr-jump"
          onSubmit={(e) => {
            e.preventDefault();
            jumpToClock(jump);
          }}
        >
          <input
            placeholder="HH:mm"
            value={jump}
            onChange={(e) => setJump(e.target.value)}
            aria-label="跳转到时间"
          />
          <button type="submit" className="btn ghost">
            跳转
          </button>
        </form>
        <button type="button" className="btn ghost" onClick={() => { markUserAction(); goPrevNext(-1); }}>
          上一条
        </button>
        <button type="button" className="btn ghost" onClick={() => { markUserAction(); goPrevNext(1); }}>
          下一条
        </button>
        <button
          type="button"
          className="btn ghost"
          onClick={() => {
            markUserAction();
            const d = new Date();
            moveWindow(d.getHours() * 60 + d.getMinutes(), 120);
          }}
        >
          现在
        </button>
      </div>

      <div className="nvr-hours" role="listbox" aria-label="按小时索引">
        {hourDensity.map((d, h) => (
          <button
            key={h}
            type="button"
            className={Math.floor(focusMin / 60) === h ? "hour on" : "hour"}
            title={`${h}:00 · 密度 ${Math.round(d * 100)}%`}
            onClick={() => {
              markUserAction();
              const lo = day.getTime() / 1000 + h * 3600;
              const hi = lo + 3600;
              const hit =
                filtered.find((e) => e.wall_ts >= lo && e.wall_ts < hi) ||
                eventsRef.current.find((e) => e.wall_ts >= lo && e.wall_ts < hi);
              if (hit) {
                cursorTsRef.current = hit.wall_ts;
                const min = Math.round((hit.wall_ts * 1000 - day.getTime()) / 60000);
                moveWindow(min, 90);
                const i = events.findIndex(
                  (e) => e.wall_ts === hit.wall_ts && e.type === hit.type,
                );
                const id = eventId(hit, i >= 0 ? i : 0);
                setSelectedId(id);
                setSelectedEv(hit);
                tlRef.current?.setSelection([id]);
              } else {
                moveWindow(h * 60 + 30, 120);
              }
            }}
          >
            <i style={{ height: `${Math.max(8, d * 100)}%` }} />
            <span>{h}</span>
          </button>
        ))}
      </div>

      <div className="vis-host" ref={hostRef} />

      <div className="nvr-legend">
        <span>
          匹配 <b>{filtered.length}</b>
        </span>
        <span>
          打字 <b>{typingCount}</b>
        </span>
        <span>
          剪贴板 <b>{clipCount}</b>
        </span>
        <span>
          点击 <b>{clickCount}</b>
        </span>
        <span>
          窗内 <b>{nearby.length}</b>
        </span>
      </div>

      {selectedEv ? (
        <div className="nvr-detail">
          <h3>
            {format(new Date(selectedEv.wall_ts * 1000), "HH:mm:ss")} · {selectedEv.type}
            {selectedEv.app?.name ? ` · ${selectedEv.app.name}` : ""}
          </h3>
          <pre>{summarize(selectedEv)}</pre>
        </div>
      ) : (
        <div className="nvr-detail">
          <h3>详情</h3>
          <pre className="muted">点击列表或时间轴条目查看完整内容（优先看「打字」）</pre>
        </div>
      )}

      <ul className="nvr-list" ref={listRef}>
        {pageItems.length === 0 ? (
          <li className="empty">该时段暂无结果 — 点小时条或改筛选。</li>
        ) : (
          pageItems.map((ev, i) => {
            const global = events.indexOf(ev);
            const id = eventId(ev, global >= 0 ? global : i);
            return (
              <li
                key={id}
                className={`ev-${ev.type}${selectedId === id ? " sel" : ""}`}
                onClick={() => {
                  markUserAction();
                  setSelectedId(id);
                  setSelectedEv(ev);
                  tlRef.current?.setSelection([id]);
                  const min = Math.round((ev.wall_ts * 1000 - day.getTime()) / 60000);
                  moveWindow(min, 60);
                }}
              >
                <time>{format(new Date(ev.wall_ts * 1000), "HH:mm:ss")}</time>
                <div>
                  <code>{ev.type}</code>
                  {ev.app?.name ? <b>{" "}{ev.app.name}</b> : null}
                  <p>{summarize(ev)}</p>
                </div>
              </li>
            );
          })
        )}
      </ul>
      {nearby.length > pageSize ? (
        <div className="nvr-pager">
          <button
            type="button"
            className="btn ghost"
            disabled={pageSafe <= 0}
            onClick={() => { markUserAction(); setListPage((p) => Math.max(0, p - 1)); }}
          >
            上一页
          </button>
          <span>
            {pageSafe + 1} / {pageCount}
          </span>
          <button
            type="button"
            className="btn ghost"
            disabled={pageSafe >= pageCount - 1}
            onClick={() => { markUserAction(); setListPage((p) => Math.min(pageCount - 1, p + 1)); }}
          >
            下一页
          </button>
        </div>
      ) : null}
    </div>
  );
}

export const DayTimeline24 = memo(DayTimeline24Inner, (a, b) => {
  if (a.date !== b.date) return false;
  if (a.events.length !== b.events.length) return false;
  if (a.events.length === 0) return true;
  const la = a.events[a.events.length - 1];
  const lb = b.events[b.events.length - 1];
  return la.wall_ts === lb.wall_ts && la.type === lb.type;
});
