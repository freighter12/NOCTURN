// Shared helpers for process priority and CPU affinity controls
// (Processes page and per-game profiles).

import type { Priority } from "./api";

export const PRIORITIES: { value: Priority; label: string; hint: string }[] = [
  { value: "idle", label: "Low", hint: "Runs only when nothing else needs the CPU." },
  { value: "below_normal", label: "Below normal", hint: "Good for background work you don't want competing with games." },
  { value: "normal", label: "Normal", hint: "Windows' default for almost every app." },
  { value: "above_normal", label: "Above normal", hint: "Gets CPU time ahead of normal apps. Usually enough for games." },
  { value: "high", label: "High", hint: "Can make the rest of Windows sluggish under heavy load. Use sparingly." },
];

/** Set bit indexes of a hex (or decimal) CPU mask string. */
export function maskBits(mask: string | null | undefined): Set<number> {
  const out = new Set<number>();
  if (!mask) return out;
  let v = BigInt(mask);
  for (let i = 0; v > 0n; i++, v >>= 1n) if (v & 1n) out.add(i);
  return out;
}

export function toMask(bits: Set<number>): string {
  let v = 0n;
  for (const b of bits) v |= 1n << BigInt(b);
  return `0x${v.toString(16)}`;
}
