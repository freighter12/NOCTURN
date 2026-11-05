// Time-series line chart.
// Specs: 2px lines, ~10% area wash, hairline solid gridlines, one y-axis,
// crosshair that snaps to the nearest sample, one tooltip listing every
// series (value first, line-key, then name), legend for >= 2 series.
// Labels are rendered as text nodes (never innerHTML).

import { useEffect, useMemo, useRef, useState } from "react";

export interface Series {
  key: string;
  label: string;
  color: string;
  values: (number | null)[];
}

interface Props {
  series: Series[];
  times: number[];
  height?: number;
  /** Fixed max (e.g. 100 for percentages). Otherwise derived from data. */
  max?: number;
  format: (v: number) => string;
  ticks?: number;
  area?: boolean;
  ariaLabel: string;
  /** Tooltip time label; defaults to "Ns ago" for live charts. */
  timeLabel?: (ts: number) => string;
}

function niceMax(v: number): number {
  if (v <= 0) return 1;
  const exp = Math.pow(10, Math.floor(Math.log10(v)));
  const n = v / exp;
  const step = n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10;
  return step * exp;
}

export function LineChart({ series, times, height = 120, max, format, ticks = 2, area = true, ariaLabel, timeLabel }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(400);
  const [hover, setHover] = useState<number | null>(null);

  useEffect(() => {
    if (!ref.current) return;
    const ro = new ResizeObserver(([e]) => setWidth(Math.max(120, e.contentRect.width)));
    ro.observe(ref.current);
    return () => ro.disconnect();
  }, []);

  const padL = 34;
  const padR = 6;
  const padT = 6;
  const padB = 6;
  const n = times.length;
  const yMax = useMemo(() => {
    if (max != null) return max;
    const m = Math.max(0, ...series.flatMap((s) => s.values.filter((v): v is number => v != null)));
    return niceMax(m * 1.1);
  }, [series, max]);

  const x = (i: number) => padL + (n <= 1 ? 0 : (i / (n - 1)) * (width - padL - padR));
  const y = (v: number) => padT + (1 - Math.min(v, yMax) / yMax) * (height - padT - padB);

  const paths = series.map((s) => {
    let d = "";
    let a = "";
    let started = false;
    let firstX = 0;
    let lastX = 0;
    s.values.forEach((v, i) => {
      if (v == null) return;
      const px = x(i);
      const py = y(v);
      if (!started) {
        d += `M${px},${py}`;
        firstX = px;
        started = true;
      } else d += `L${px},${py}`;
      lastX = px;
    });
    if (started) a = `${d}L${lastX},${y(0)}L${firstX},${y(0)}Z`;
    return { s, d, a };
  });

  const tickVals = Array.from({ length: ticks + 1 }, (_, i) => (yMax / ticks) * i);

  function onMove(e: React.PointerEvent<SVGSVGElement>) {
    if (n < 2) return;
    const rect = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
    const px = e.clientX - rect.left;
    const i = Math.round(((px - padL) / (width - padL - padR)) * (n - 1));
    setHover(Math.max(0, Math.min(n - 1, i)));
  }

  const hx = hover != null ? x(hover) : 0;
  const secondsAgo = hover != null && n ? Math.round((times[n - 1] - times[hover]) / 1000) : 0;

  return (
    <div className="chart" ref={ref}>
      <svg
        height={height}
        role="img"
        aria-label={ariaLabel}
        onPointerMove={onMove}
        onPointerLeave={() => setHover(null)}
        style={{ touchAction: "none" }}
      >
        {tickVals.map((t, i) => (
          <g key={i}>
            <line x1={padL} x2={width - padR} y1={y(t)} y2={y(t)} stroke={i === 0 ? "var(--axis)" : "var(--grid)"} strokeWidth={1} shapeRendering="crispEdges" />
            <text x={padL - 8} y={y(t)} dy="0.32em" textAnchor="end" fill="var(--ink-3)" fontSize="10.5" style={{ fontVariantNumeric: "tabular-nums" }}>
              {format(t)}
            </text>
          </g>
        ))}
        {area &&
          paths.map(({ s, a }) => a && <path key={`a-${s.key}`} d={a} fill={s.color} opacity={0.1} />)}
        {paths.map(({ s, d }) => d && <path key={s.key} d={d} fill="none" stroke={s.color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />)}
        {hover != null && (
          <g>
            <line x1={hx} x2={hx} y1={padT} y2={height - padB} stroke="var(--line-3)" strokeWidth={1} />
            {series.map((s) => {
              const v = s.values[hover];
              return v == null ? null : <circle key={s.key} cx={hx} cy={y(v)} r={4} fill={s.color} stroke="var(--bg-2)" strokeWidth={2} />;
            })}
          </g>
        )}
        {/* transparent hit area covering the full plot */}
        <rect x={padL} y={0} width={Math.max(0, width - padL - padR)} height={height} fill="transparent" />
      </svg>
      {hover != null && (
        <div className="tip" style={{ left: Math.min(Math.max(hx, 70), width - 70), top: padT }}>
          <div className="t">{timeLabel && hover != null ? timeLabel(times[hover]) : secondsAgo === 0 ? "Now" : `${secondsAgo}s ago`}</div>
          {series.map((s) => {
            const v = s.values[hover];
            return (
              <div className="r" key={s.key}>
                <span className="linekey" style={{ background: s.color }} />
                <b>{v == null ? "—" : format(v)}</b>
                <span>{s.label}</span>
              </div>
            );
          })}
        </div>
      )}
      {series.length > 1 && (
        <div className="legend mt-1">
          {series.map((s) => (
            <span className="item" key={s.key}>
              <span className="linekey" style={{ background: s.color }} />
              {s.label}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
