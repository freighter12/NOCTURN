// Sky for the Home header: tonight's moon phase (computed locally from the
// date — no network) and the time-of-day palette.

/** Average length of the lunar cycle in days. */
export const SYNODIC = 29.530588853;
/** A known new moon: 2000-01-06 18:14 UTC. */
const NEW_MOON_MS = Date.UTC(2000, 0, 6, 18, 14);

export interface MoonPhase {
  /** 0 = new, 0.25 = first quarter, 0.5 = full, 0.75 = last quarter. */
  phase: number;
  /** Lit fraction of the disc, 0–1. */
  illumination: number;
  waxing: boolean;
  name: string;
  /** Days until the next full moon. */
  daysToFull: number;
}

export function moonPhase(date = new Date()): MoonPhase {
  const days = (date.getTime() - NEW_MOON_MS) / 86_400_000;
  const phase = (((days / SYNODIC) % 1) + 1) % 1;
  const illumination = (1 - Math.cos(2 * Math.PI * phase)) / 2;
  const names = ["New moon", "Waxing crescent", "First quarter", "Waxing gibbous", "Full moon", "Waning gibbous", "Last quarter", "Waning crescent"];
  const name = names[Math.round(phase * 8) % 8];
  const toFull = (((0.5 - phase) % 1) + 1) % 1;
  return { phase, illumination, waxing: phase < 0.5, name, daysToFull: toFull * SYNODIC };
}

/**
 * SVG path for the lit part of a moon of radius r centred at (cx, cy), as
 * seen from the northern hemisphere (waxing = lit on the right). Outer edge
 * is a half circle on the lit side; the terminator is a half ellipse whose
 * width follows the phase.
 */
export function litPath(phase: number, cx: number, cy: number, r: number): string {
  const k = Math.cos(2 * Math.PI * phase); // 1 new … -1 full
  if (Math.abs(1 - k) < 1e-3) return ""; // new moon: nothing lit
  const waxing = phase < 0.5;
  const rx = Math.abs(k) * r;
  const top = `${cx} ${cy - r}`;
  const bottom = `${cx} ${cy + r}`;
  const limbSweep = waxing ? 1 : 0; // right half when waxing, left when waning
  // Crescent (k > 0): terminator curves towards the lit side; gibbous: away from it.
  const termSweep = (k > 0) === waxing ? 0 : 1;
  return `M ${top} A ${r} ${r} 0 0 ${limbSweep} ${bottom} A ${rx.toFixed(2)} ${r} 0 0 ${termSweep} ${top} Z`;
}

export type SkyTime = "night" | "dawn" | "day" | "dusk";

export function skyTime(date = new Date()): SkyTime {
  const h = date.getHours() + date.getMinutes() / 60;
  if (h >= 21 || h < 5) return "night";
  if (h < 8) return "dawn";
  if (h < 17.5) return "day";
  return "dusk";
}

/** Small deterministic random generator so the stars don't jump on every render. */
export function seeded(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return ((s >>> 0) % 100000) / 100000;
  };
}
