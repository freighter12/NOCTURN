// Home header: a sky that follows the time of day, with tonight's real moon
// phase drawn in the logo's style, twinkling stars at night and the odd
// shooting star. Pure SVG/CSS; still with reduce motion.

import { useEffect, useMemo, useState, type ReactNode } from "react";
import { usePrefs } from "./PrefsProvider";
import { litPath, moonPhase, seeded, skyTime, type SkyTime } from "./sky";

const STARS: Record<SkyTime, number> = { night: 70, dusk: 34, dawn: 22, day: 0 };

export function SkyHeader({ eyebrow, title, children, actions, sky = "auto", showMoon = true, showClock = false, showDate = false }: { eyebrow: string; title: string; children?: ReactNode; actions?: ReactNode; sky?: SkyTime | "auto"; showMoon?: boolean; showClock?: boolean; showDate?: boolean }) {
  const { prefs } = usePrefs();
  const reduce = prefs.accessibility.reduceMotion || document.body.classList.contains("reduce-motion");
  // Re-check every 5 minutes so the sky moves on through the day.
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    // With a clock showing, tick on the minute; otherwise every 5 minutes.
    const every = showClock ? 60_000 : 5 * 60_000;
    let t = 0;
    const first = window.setTimeout(() => {
      setNow(new Date());
      t = window.setInterval(() => setNow(new Date()), every);
    }, showClock ? 60_000 - (Date.now() % 60_000) + 50 : every);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(t);
    };
  }, [showClock]);
  // Dev builds only: preview another time of day (localStorage "nocturn.dev.sky" = night|dawn|day|dusk).
  const devSky = import.meta.env.DEV ? (() => { try { return window.localStorage.getItem("nocturn.dev.sky") as SkyTime | null; } catch { return null; } })() : null;
  const time = devSky && devSky in STARS ? devSky : sky !== "auto" ? sky : skyTime(now);
  const moon = useMemo(() => moonPhase(now), [now]);
  const stars = useMemo(() => {
    const r = seeded(20261107);
    return Array.from({ length: 70 }, () => ({ x: r() * 100, y: r() * 88, s: 0.6 + r() * 1.5, d: r() * 6, dur: 2.5 + r() * 4, bright: r() > 0.86 }));
  }, []);

  const R = 34;
  const cx = 50;
  const cy = 50;
  const lit = litPath(moon.phase, cx, cy, R);
  const pct = Math.round(moon.illumination * 100);
  const fullIn = moon.daysToFull < 1 ? "tonight" : `in ${Math.round(moon.daysToFull)} day${Math.round(moon.daysToFull) === 1 ? "" : "s"}`;

  return (
    <header className={`sky sky-${time} ${reduce ? "still" : ""}`}>
      <div className="sky-stars" aria-hidden>
        {stars.slice(0, STARS[time]).map((s, i) => (
          <i key={i} className={s.bright ? "b" : ""} style={{ left: `${s.x}%`, top: `${s.y}%`, width: s.s, height: s.s, animationDelay: `${s.d}s`, animationDuration: `${s.dur}s` }} />
        ))}
        {time === "night" && !reduce && <span className="shooting" />}
      </div>
      <div className="sky-horizon" aria-hidden />
      {showClock && (
        <div className="sky-clock" aria-label={now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}>
          <span className="num">{now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
          {showDate && <span className="sky-date">{now.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}</span>}
        </div>
      )}
      {showMoon && <div className="sky-moon" aria-hidden title={`${moon.name}, ${pct}% lit`}>
        <svg viewBox="0 0 100 100">
          <defs>
            <linearGradient id="sky-moon-g" x1="0.2" y1="0.1" x2="0.8" y2="0.95">
              <stop offset="0" stopColor="#F4F5F8" />
              <stop offset="0.55" stopColor="#C9CCD4" />
              <stop offset="1" stopColor="#8A8E9A" />
            </linearGradient>
            <radialGradient id="sky-moon-glow" cx="50%" cy="50%" r="50%">
              <stop offset="0.5" stopColor="#dfe6f5" stopOpacity={0.12 + moon.illumination * 0.2} />
              <stop offset="1" stopColor="#dfe6f5" stopOpacity="0" />
            </radialGradient>
          </defs>
          <circle cx={cx} cy={cy} r="50" fill="url(#sky-moon-glow)" />
          {/* earthshine: the unlit part, faintly */}
          <circle cx={cx} cy={cy} r={R} fill="#1d2029" opacity="0.85" />
          {lit && <path d={lit} fill="url(#sky-moon-g)" />}
        </svg>
      </div>}
      <div className="sky-text">
        <div className="sky-eyebrow">{eyebrow}</div>
        <h1>{title}</h1>
        {children && <div className="sky-desc">{children}</div>}
        {!showClock && showDate && <div className="sky-moon-line">{now.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}</div>}
        {showMoon && (
          <div className="sky-moon-line">
            {moon.name} · {pct}% lit{moon.name !== "Full moon" ? ` · full moon ${fullIn}` : ""}
          </div>
        )}
      </div>
      {actions && <div className="sky-actions">{actions}</div>}
    </header>
  );
}
