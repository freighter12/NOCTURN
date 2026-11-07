// Command palette model: what a command is, how a query matches it, and
// how results are ordered. Kept free of React so it can be unit-tested.

export type CommandGroup = "Recent" | "Suggested" | "Pages" | "Actions" | "Apps" | "Workspaces" | "Music" | "Themes" | "Settings" | "Ask";

export interface Command {
  /** Stable ID, used to remember recent commands. */
  id: string;
  group: CommandGroup;
  label: string;
  /** Shown dimmed after the label. */
  hint?: string;
  /** Extra words that should find this command. */
  keywords?: string[];
  icon?: string;
  /** Optional text badge shown instead of an icon (e.g. an app's initials). */
  badge?: { text: string; colour?: string | null };
  run: () => void;
}

const norm = (s: string) => s.toLowerCase().normalize("NFKD").replace(/[̀-ͯ]/g, "");

/**
 * How well `query` matches `text`, 0 = not at all. Higher is better:
 * whole text > prefix > word prefix > substring > in-order letters.
 */
export function matchScore(query: string, text: string): number {
  const q = norm(query.trim());
  if (!q) return 1;
  const t = norm(text);
  if (t === q) return 100;
  if (t.startsWith(q)) return 80 - Math.min(20, t.length - q.length) / 2;
  const words = t.split(/[^a-z0-9]+/).filter(Boolean);
  // Every query word starts some word of the text ("clean temp" → "Clean temporary files").
  const qw = q.split(/\s+/).filter(Boolean);
  if (qw.every((w) => words.some((x) => x.startsWith(w)))) return 60 - Math.min(10, words.length);
  if (t.includes(q)) return 45;
  // Initials ("ws" → "Windows settings").
  const initials = words.map((w) => w[0]).join("");
  if (initials.startsWith(q)) return 40;
  // Letters in order, rewarding tight runs.
  let i = 0;
  let gaps = 0;
  let last = -1;
  for (let j = 0; j < t.length && i < q.length; j++) {
    if (t[j] === q[i]) {
      if (last >= 0 && j - last > 1) gaps++;
      last = j;
      i++;
    }
  }
  if (i < q.length || q.length < 2) return 0;
  return Math.max(1, 25 - gaps * 4);
}

/** Best score across a command's label and keywords (keywords count a little less). */
export function commandScore(query: string, c: Command): number {
  let best = matchScore(query, c.label);
  for (const k of c.keywords ?? []) best = Math.max(best, matchScore(query, k) * 0.85);
  if (c.hint) best = Math.max(best, matchScore(query, c.hint) * 0.5);
  return best;
}

const GROUP_ORDER: CommandGroup[] = ["Recent", "Suggested", "Pages", "Actions", "Apps", "Workspaces", "Music", "Themes", "Settings", "Ask"];

/**
 * Rank commands for a query. With no query: recent commands first, then the
 * given suggestions. With a query: by score (recently used ones nudged up),
 * capped per group so one big group can't push everything else out.
 */
export function rank(query: string, commands: Command[], recentIds: string[], opts: { perGroup?: number; suggestions?: string[] } = {}): Command[] {
  const perGroup = opts.perGroup ?? 6;
  const byId = new Map(commands.map((c) => [c.id, c]));
  if (!query.trim()) {
    const recent = recentIds.map((id) => byId.get(id)).filter((c): c is Command => !!c).slice(0, 5);
    const seen = new Set(recent.map((c) => c.id));
    const sugg = (opts.suggestions ?? []).map((id) => byId.get(id)).filter((c): c is Command => !!c && !seen.has(c.id));
    return [...recent.map((c) => ({ ...c, group: "Recent" as const })), ...sugg.map((c) => ({ ...c, group: "Suggested" as const }))];
  }
  const scored = commands
    .map((c) => {
      const s = commandScore(query, c);
      const r = recentIds.indexOf(c.id);
      return { c, s: s > 0 ? s + (r >= 0 ? 8 - Math.min(7, r) : 0) : 0 };
    })
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s || a.c.label.localeCompare(b.c.label));
  const counts = new Map<CommandGroup, number>();
  const out: Command[] = [];
  for (const { c } of scored) {
    const n = counts.get(c.group) ?? 0;
    if (n >= perGroup) continue;
    counts.set(c.group, n + 1);
    out.push(c);
  }
  // Results are shown under group headings: the group holding the best match
  // comes first, the rest follow their own best match (ties: fixed order).
  // `out` is already sorted by score, so first appearance = group's best.
  const groupRank = new Map<CommandGroup, number>();
  out.forEach((c, i) => groupRank.has(c.group) || groupRank.set(c.group, i));
  return out
    .map((c, i) => ({ c, i }))
    .sort((a, b) => groupRank.get(a.c.group)! - groupRank.get(b.c.group)! || GROUP_ORDER.indexOf(a.c.group) - GROUP_ORDER.indexOf(b.c.group) || a.i - b.i)
    .map((x) => x.c);
}

/** Move an ID to the front of the recent list (max 8). */
export function pushRecent(list: string[], id: string): string[] {
  return [id, ...list.filter((x) => x !== id)].slice(0, 8);
}
