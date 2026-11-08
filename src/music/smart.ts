// Smart playlists: built-in mixes made from the user's own listening (on
// this PC), and rule-based lists the user defines. Pure functions so they
// can be unit-tested; lists are worked out from the library each time, so
// they keep themselves up to date.

import type { MusicStats, Track } from "../lib/api";

// ------------------------------------------------------------------ rules

export type TextField = "title" | "artist" | "album" | "genre";
export type NumField = "year" | "plays" | "duration";
export type DateField = "added" | "played";
export type Field = TextField | NumField | DateField | "liked" | "source";

export type Rule =
  | { field: TextField; op: "contains" | "is" | "not_contains" | "starts"; value: string }
  | { field: NumField; op: "gt" | "lt" | "eq"; value: number }
  | { field: DateField; op: "within" | "older" | "never"; value: number }
  | { field: "liked"; op: "is" | "not"; value: boolean }
  | { field: "source"; op: "is"; value: "local" | "youtube" };

export type SmartSort = "plays" | "recent" | "added" | "title" | "random";

export interface SmartRules {
  match: "all" | "any";
  rules: Rule[];
  sort: SmartSort;
  /** Most songs to include (null = all). */
  limit: number | null;
}

export const TEXT_FIELDS: TextField[] = ["title", "artist", "album", "genre"];
export const NUM_FIELDS: NumField[] = ["year", "plays", "duration"];
export const DATE_FIELDS: DateField[] = ["added", "played"];
export const FIELD_LABEL: Record<Field, string> = {
  title: "Title", artist: "Artist", album: "Album", genre: "Genre", year: "Year", plays: "Plays", duration: "Length (minutes)",
  added: "Added", played: "Last played", liked: "Liked", source: "Source",
};
export const SORT_LABEL: Record<SmartSort, string> = { plays: "Most played", recent: "Recently played", added: "Recently added", title: "Title", random: "Shuffled (daily)" };

export const defaultRule = (field: Field = "genre"): Rule => {
  if ((TEXT_FIELDS as string[]).includes(field)) return { field: field as TextField, op: "contains", value: "" };
  if ((NUM_FIELDS as string[]).includes(field)) return { field: field as NumField, op: "gt", value: field === "year" ? 2000 : field === "duration" ? 5 : 3 };
  if ((DATE_FIELDS as string[]).includes(field)) return { field: field as DateField, op: "within", value: 30 };
  if (field === "liked") return { field: "liked", op: "is", value: true };
  return { field: "source", op: "is", value: "local" };
};

export const DEFAULT_RULES: SmartRules = { match: "all", rules: [defaultRule("genre")], sort: "plays", limit: 100 };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const finite = (v: unknown, d: number, min: number, max: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : d);

/** Whatever was stored → valid rules (bad rules are dropped). */
export function sanitizeRules(raw: unknown): SmartRules {
  const r = isObj(raw) ? raw : {};
  const rules: Rule[] = [];
  for (const x of Array.isArray(r.rules) ? r.rules.slice(0, 20) : []) {
    if (!isObj(x) || typeof x.field !== "string") continue;
    const f = x.field as Field;
    if ((TEXT_FIELDS as string[]).includes(f)) {
      const op = ["contains", "is", "not_contains", "starts"].includes(x.op as string) ? (x.op as "contains") : "contains";
      rules.push({ field: f as TextField, op, value: typeof x.value === "string" ? x.value.slice(0, 100) : "" });
    } else if ((NUM_FIELDS as string[]).includes(f)) {
      const op = ["gt", "lt", "eq"].includes(x.op as string) ? (x.op as "gt") : "gt";
      rules.push({ field: f as NumField, op, value: finite(x.value, 0, -1e6, 1e6) });
    } else if ((DATE_FIELDS as string[]).includes(f)) {
      const op = ["within", "older", "never"].includes(x.op as string) ? (x.op as "within" | "older" | "never") : "within";
      rules.push({ field: f as DateField, op: f === "added" && op === "never" ? "within" : op, value: Math.round(finite(x.value, 30, 0, 3650)) });
    } else if (f === "liked") {
      rules.push({ field: "liked", op: x.op === "not" ? "not" : "is", value: true });
    } else if (f === "source") {
      rules.push({ field: "source", op: "is", value: x.value === "youtube" ? "youtube" : "local" });
    }
  }
  return {
    match: r.match === "any" ? "any" : "all",
    rules,
    sort: (Object.keys(SORT_LABEL) as SmartSort[]).includes(r.sort as SmartSort) ? (r.sort as SmartSort) : "plays",
    limit: r.limit === null ? null : Math.round(finite(r.limit, 100, 1, 5000)),
  };
}

const DAY = 86_400_000;
const lc = (s: string) => s.toLocaleLowerCase();

export function matches(t: Track, r: Rule, now: number): boolean {
  switch (r.field) {
    case "title":
    case "artist":
    case "album":
    case "genre": {
      const have = lc(t[r.field] ?? "");
      const want = lc(r.value.trim());
      if (!want) return true;
      if (r.op === "is") return have === want;
      if (r.op === "starts") return have.startsWith(want);
      if (r.op === "not_contains") return !have.includes(want);
      return have.includes(want);
    }
    case "year":
    case "plays":
    case "duration": {
      const v = r.field === "year" ? t.year : r.field === "plays" ? t.plays : t.duration_ms / 60_000;
      if (v == null) return false;
      return r.op === "gt" ? v > r.value : r.op === "lt" ? v < r.value : Math.round(v) === r.value;
    }
    case "added":
    case "played": {
      const when = r.field === "added" ? t.added_at : t.last_played;
      if (r.op === "never") return !when;
      if (!when) return r.op === "older" && r.field === "played";
      const age = (now - Date.parse(when)) / DAY;
      return r.op === "within" ? age <= r.value : age > r.value;
    }
    case "liked":
      return r.op === "is" ? t.liked : !t.liked;
    case "source":
      return t.source === r.value;
  }
}

/** Small seeded shuffle, so "daily" lists keep their order all day. */
export function seededShuffle<T>(xs: T[], seed: string): T[] {
  let h = 2166136261;
  for (const c of seed) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  const rand = () => {
    h = Math.imul(h ^ (h >>> 15), 2246822507);
    h = Math.imul(h ^ (h >>> 13), 3266489909);
    return ((h ^= h >>> 16) >>> 0) / 4294967296;
  };
  const a = [...xs];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

const today = (now: number) => new Date(now).toDateString();

function sortTracks(ts: Track[], sort: SmartSort, seed: string, now: number): Track[] {
  const by = <K>(f: (t: Track) => K, desc = true) => [...ts].sort((a, b) => {
    const x = f(a), y = f(b);
    return x === y ? 0 : (x > y ? 1 : -1) * (desc ? -1 : 1);
  });
  switch (sort) {
    case "plays": return by((t) => t.plays);
    case "recent": return by((t) => t.last_played ?? "");
    case "added": return by((t) => t.added_at);
    case "title": return [...ts].sort((a, b) => a.title.localeCompare(b.title));
    case "random": return seededShuffle(ts, `${seed}|${today(now)}`);
  }
}

/** The songs a rule set picks, in order. */
export function evaluate(rules: SmartRules, tracks: Track[], now = Date.now(), seed = ""): number[] {
  const pick = tracks.filter((t) => (rules.rules.length === 0 ? true : rules.match === "all" ? rules.rules.every((r) => matches(t, r, now)) : rules.rules.some((r) => matches(t, r, now))));
  const sorted = sortTracks(pick, rules.sort, seed, now);
  return (rules.limit ? sorted.slice(0, rules.limit) : sorted).map((t) => t.id);
}

// ------------------------------------------------------------- built-ins

export interface SmartList {
  id: string;
  name: string;
  description: string;
  glyph: string;
  colour: string;
  ids: number[];
  /** Set for the user's own lists. */
  rules?: SmartRules;
}

/** What to call the mix for this hour. */
export function timeOfDay(hour: number): { key: string; name: string; glyph: string } {
  if (hour >= 5 && hour < 12) return { key: "morning", name: "Morning mix", glyph: "☀" };
  if (hour >= 12 && hour < 17) return { key: "afternoon", name: "Afternoon mix", glyph: "◐" };
  if (hour >= 17 && hour < 22) return { key: "evening", name: "Evening mix", glyph: "◑" };
  return { key: "night", name: "Late-night mix", glyph: "☾" };
}

/** How often each song is played within ±`span` hours of `hour` (closer counts more). */
export function hourScores(stats: MusicStats, hour: number, span = 2): Map<number, number> {
  const out = new Map<number, number>();
  for (const [id, h, n] of stats.by_hour) {
    const d = Math.min(Math.abs(h - hour), 24 - Math.abs(h - hour));
    if (d > span) continue;
    out.set(id, (out.get(id) ?? 0) + n * (1 - d / (span + 1)));
  }
  return out;
}

const CALM = /ambient|chill|lo-?fi|acoustic|piano|classical|jazz|sleep|calm|downtempo|folk|soul|r&b|rnb|soundtrack|score|new age|instrumental|bossa|meditat|slowed|lullab/i;

export const BUILTIN_IDS = ["builtin:time", "builtin:repeat", "builtin:forgotten", "builtin:winddown", "builtin:most", "builtin:new", "builtin:unplayed"] as const;

/** All built-in mixes that have enough songs to be worth showing. */
export function builtins(tracks: Track[], stats: MusicStats | null, now = Date.now()): SmartList[] {
  const byId = new Map(tracks.map((t) => [t.id, t]));
  const day = today(now);
  const hour = new Date(now).getHours();
  const out: SmartList[] = [];
  const add = (l: Omit<SmartList, "ids"> & { ids: number[] }, min = 5) => {
    const ids = l.ids.filter((id) => byId.has(id));
    if (ids.length >= min) out.push({ ...l, ids });
  };

  // For this time of day: what the user tends to play around now.
  const tod = timeOfDay(hour);
  if (stats) {
    const scores = hourScores(stats, hour);
    const top = [...scores.entries()].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([id]) => id);
    // Liked songs by the same artists fill it out.
    const artists = new Set(top.map((id) => byId.get(id)?.artist).filter(Boolean));
    const more = tracks.filter((t) => t.liked && artists.has(t.artist) && !top.includes(t.id)).map((t) => t.id);
    add({ id: "builtin:time", name: tod.name, description: `What you usually play around this time, shuffled fresh each day.`, glyph: tod.glyph, colour: "#2f3b5c", ids: seededShuffle([...top, ...more.slice(0, 20)], `time|${tod.key}|${day}`) });
  }

  if (stats) {
    const recent = [...stats.recent].filter(([, n]) => n >= 2).sort((a, b) => b[1] - a[1]).slice(0, 30).map(([id]) => id);
    add({ id: "builtin:repeat", name: "On repeat", description: "Your most played songs of the last 30 days.", glyph: "↻", colour: "#4a3b6b", ids: recent });
  }

  const forgotten = tracks
    .filter((t) => (t.liked || t.plays >= 5) && (t.last_played ? now - Date.parse(t.last_played) > 60 * DAY : t.liked))
    .sort((a, b) => b.plays - a.plays)
    .slice(0, 50)
    .map((t) => t.id);
  add({ id: "builtin:forgotten", name: "Forgotten favourites", description: "Songs you loved but haven't played in two months or more.", glyph: "♡", colour: "#6b3b4a", ids: forgotten });

  // Calm songs for winding down: by genre, plus what the user plays late at night.
  const late = stats ? [...hourScores(stats, 0, 3).entries()].sort((a, b) => b[1] - a[1]).map(([id]) => id) : [];
  const calm = tracks.filter((t) => CALM.test(t.genre)).map((t) => t.id);
  const wind = [...new Set([...calm, ...late.slice(0, 40)])].slice(0, 80);
  add({ id: "builtin:winddown", name: "Wind-down mix", description: "Calmer songs and what you play late at night — for the end of the day.", glyph: "☾", colour: "#1f2a44", ids: seededShuffle(wind, `wind|${day}`) });

  add({ id: "builtin:most", name: "Most played", description: "Your all-time most played songs.", glyph: "★", colour: "#5c4a2f", ids: tracks.filter((t) => t.plays >= 3).sort((a, b) => b.plays - a.plays).slice(0, 50).map((t) => t.id) });

  add({ id: "builtin:new", name: "New to your library", description: "Added in the last 30 days, newest first.", glyph: "+", colour: "#2f5c4a", ids: tracks.filter((t) => now - Date.parse(t.added_at) <= 30 * DAY).sort((a, b) => b.added_at.localeCompare(a.added_at)).slice(0, 100).map((t) => t.id) });

  add({ id: "builtin:unplayed", name: "Never played", description: "Songs in your library you haven't listened to yet.", glyph: "?", colour: "#3b4a5c", ids: seededShuffle(tracks.filter((t) => t.plays === 0).map((t) => t.id), `unplayed|${day}`).slice(0, 100) });

  return out;
}

/** The user's own smart lists, worked out now. */
export function customLists(records: { id: string; name: string; rules: unknown }[], tracks: Track[], now = Date.now()): SmartList[] {
  return records.map((r) => {
    const rules = sanitizeRules(r.rules);
    return { id: r.id, name: r.name, description: describe(rules), glyph: "✦", colour: "#3b4a6b", ids: evaluate(rules, tracks, now, r.id), rules };
  });
}

/** Plain-English summary: "Genre contains jazz and liked · most played · up to 100". */
export function describe(r: SmartRules): string {
  const one = (x: Rule): string => {
    const f = FIELD_LABEL[x.field].toLowerCase();
    switch (x.field) {
      case "liked": return x.op === "is" ? "liked" : "not liked";
      case "source": return x.value === "youtube" ? "streamed" : "your own files";
      case "added": return x.op === "within" ? `added in the last ${x.value} days` : `added over ${x.value} days ago`;
      case "played": return x.op === "never" ? "never played" : x.op === "within" ? `played in the last ${x.value} days` : `not played for ${x.value} days`;
      case "year": case "plays": case "duration": return `${f} ${x.op === "gt" ? "over" : x.op === "lt" ? "under" : "is"} ${x.value}`;
      default: return `${f} ${x.op === "is" ? "is" : x.op === "starts" ? "starts with" : x.op === "not_contains" ? "doesn't contain" : "contains"} "${x.value}"`;
    }
  };
  const rules = r.rules.length ? r.rules.map(one).join(r.match === "all" ? " and " : " or ") : "every song";
  return `${rules[0].toUpperCase()}${rules.slice(1)} · ${SORT_LABEL[r.sort].toLowerCase()}${r.limit ? ` · up to ${r.limit}` : ""}`;
}
