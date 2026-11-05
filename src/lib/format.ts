export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !isFinite(n)) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${Math.round(v)} B` : `${v.toFixed(v >= 100 ? 0 : digits)} ${units[i]}`;
}

export function bytesParts(n: number | null | undefined): [string, string] {
  const s = bytes(n);
  const i = s.lastIndexOf(" ");
  return i > 0 ? [s.slice(0, i), s.slice(i + 1)] : [s, ""];
}

export function rate(bps: number): string {
  const bits = bps * 8;
  if (bits < 1000) return `${Math.round(bits)} b/s`;
  if (bits < 1e6) return `${(bits / 1e3).toFixed(0)} Kb/s`;
  if (bits < 1e9) return `${(bits / 1e6).toFixed(bits < 1e7 ? 1 : 0)} Mb/s`;
  return `${(bits / 1e9).toFixed(2)} Gb/s`;
}

export function pct(v: number | null | undefined, digits = 0): string {
  if (v == null || !isFinite(v)) return "—";
  return `${v.toFixed(digits)}%`;
}

export function duration(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function date(iso: string | null | undefined, withTime = false): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return withTime
    ? d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
    : d.toLocaleDateString(undefined, { dateStyle: "medium" });
}

export function relative(iso: string | null | undefined): string {
  if (!iso) return "—";
  const t = new Date(iso).getTime();
  if (isNaN(t)) return iso;
  const s = Math.round((Date.now() - t) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 7) return `${Math.round(s / 86400)} d ago`;
  return date(iso);
}

export const takesLabel: Record<string, string> = {
  immediately: "Takes effect immediately",
  app_restart: "Restart the game/app",
  sign_out: "Sign out to take effect",
  restart: "Restart required",
};

export const riskLabel: Record<string, string> = {
  safe: "Safe",
  moderate: "Moderate",
  advanced: "Advanced",
};

export const statusLabel: Record<string, string> = {
  applied: "Applied",
  unchanged: "Already set",
  failed: "Failed",
  skipped: "Skipped",
  pending: "Pending",
  unknown: "Unconfirmed",
  rolled_back: "Rolled back",
  rollback_failed: "Rollback failed",
  not_reversible: "One-time action",
  completed: "Completed",
  partial: "Partially applied",
  nothing_to_do: "Nothing to change",
  interrupted: "Interrupted",
  partially_rolled_back: "Partially rolled back",
  in_progress: "In progress",
};
