import { useEffect, useRef, useState } from "react";
import { DayPicker } from "react-day-picker";
import { zhCN } from "react-day-picker/locale";
import "react-day-picker/style.css";

/**
 * 0.6.57 — 日期选择面板（react-day-picker 10，非自造轮子）。
 *
 * 用户要求：「使用优秀的日期选择模版，确保能看出来有采集的日期方便点击查看数据」。
 * 关键设计：
 *   - **有采集数据的日期**带实心圆点 + 更亮的字色；没数据的日期暗淡 ——
 *     一眼看出哪天有东西可看。
 *   - 点击任意日期即选中并关闭；今天有独立描边。
 *   - 中文界面（zhCN locale），深色主题走 CSS 变量与项目一致。
 */
type Props = {
  /** 当前选中日期 YYYY-MM-DD */
  value: string;
  /** 有采集数据的日期集合 YYYY-MM-DD */
  withData: Set<string>;
  /** 选中某天（YYYY-MM-DD） */
  onPick: (day: string) => void;
};

function iso(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function parse(s: string): Date {
  const [y, m, d] = s.split("-").map((n) => Number(n));
  return new Date(y, (m || 1) - 1, d || 1);
}

export function DayPickerPanel({ value, withData, onPick }: Props) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const selected = parse(value);

  // 点击面板外部关闭。日历与触发按钮都在 wrapRef 内，点日历不关。
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  // 有数据的日期列表（供 modifiers 用）。空集合 → 不产生标记，无副作用。
  const datesWithData = Array.from(withData, parse);
  const hasToday = withData.has(iso(new Date()));

  return (
    <div className="dp-wrap" ref={wrapRef}>
      <button
        type="button"
        className={open ? "dp-trigger on" : "dp-trigger"}
        onClick={() => setOpen((v) => !v)}
        title="选择日期：带绿点的日期有采集数据"
        aria-haspopup="dialog"
        aria-expanded={open}
      >
        <span className="dp-trigger-icon" aria-hidden>
          ▦
        </span>
        {value}
      </button>
      {open ? (
        <div className="dp-pop" role="dialog" aria-label="选择日期">
          <DayPicker
            mode="single"
            locale={zhCN}
            selected={selected}
            defaultMonth={selected}
            showOutsideDays
            onSelect={(d) => {
              if (!d) return;
              onPick(iso(d));
              setOpen(false);
            }}
            modifiers={{ hasData: datesWithData }}
            modifiersClassNames={{ hasData: "dp-has-data" }}
            classNames={{
              today: hasToday ? "dp-today" : undefined,
            }}
          />
          <div className="dp-legend">
            <span className="dp-dot" aria-hidden />
            有采集数据
          </div>
        </div>
      ) : null}
    </div>
  );
}
