// Animated page backgrounds ("scenes") behind every page: aurora ribbons, a
// turning nebula, drifting stars, a synthwave horizon, rising fireflies.
// Only transforms and opacity animate, so the GPU composites them without
// repainting; WebView2 stops drawing them while NOCTURN is hidden, and reduce
// motion freezes them in place.

import { useMemo } from "react";
import { seeded } from "./sky";
import { ANIMATED_SCENES } from "./theme";
import type { BackgroundStyle } from "./prefs";

function starShadows(n: number, seed: number, size: number) {
  const r = seeded(seed);
  return Array.from({ length: n }, () => `${Math.round(r() * size)}px ${Math.round(r() * size)}px rgba(var(--scene-star), ${(0.25 + r() * 0.6).toFixed(2)})`).join(", ");
}

export function Scene({ kind, still }: { kind: BackgroundStyle; still: boolean }) {
  const flies = useMemo(() => {
    const r = seeded(20261110);
    return Array.from({ length: 26 }, () => ({ x: r() * 100, d: r() * 14, dur: 11 + r() * 12, s: 2 + r() * 2.5, sway: 10 + r() * 30 }));
  }, []);
  const stars = useMemo(() => [starShadows(120, 7, 1600), starShadows(70, 11, 1600), starShadows(35, 13, 1600)], []);
  if (!ANIMATED_SCENES.includes(kind)) return null;
  return (
    <div className={`scene scene-${kind} ${still ? "still" : ""}`} aria-hidden>
      {kind === "aurora" && (
        <>
          <i className="ribbon r1" />
          <i className="ribbon r2" />
          <i className="ribbon r3" />
        </>
      )}
      {kind === "nebula" && (
        <>
          <i className="cloud c1" />
          <i className="cloud c2" />
          <i className="cloud c3" />
          <i className="dust" style={{ boxShadow: stars[1] }} />
        </>
      )}
      {kind === "starfield" && (
        <>
          <i className="layer l1" style={{ boxShadow: stars[0] }} />
          <i className="layer l2" style={{ boxShadow: stars[1] }} />
          <i className="layer l3" style={{ boxShadow: stars[2] }} />
          <i className="haze" />
        </>
      )}
      {kind === "waves" && (
        <>
          <i className="sun" />
          <div className="floor">
            <i className="grid" />
          </div>
          <i className="glow" />
        </>
      )}
      {kind === "fireflies" && (
        <>
          <i className="meadow" />
          {flies.map((f, i) => (
            <i
              key={i}
              className="fly"
              style={{ left: `${f.x}%`, width: f.s, height: f.s, animationDelay: `-${f.d}s`, animationDuration: `${f.dur}s`, ["--sway" as string]: `${f.sway}px` }}
            />
          ))}
        </>
      )}
    </div>
  );
}
