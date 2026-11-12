// Nocturn Wrapped: listening time, top songs and artists, from the log the
// native player keeps (plugins/nocturn/ios/Sources/Stats.swift).
import type { Track } from "./api";

export type StatsSong = { ms: number; plays: number };
export type StatsDay = { ms: number; hours: number[]; songs: Record<string, StatsSong> };
export type StatsMeta = { title: string; artist: string; album?: string | null; artwork?: string | null };
export type StatsFile = { days: Record<string, StatsDay>; tracks: Record<string, StatsMeta>; since?: string | null };

export type Period = "week" | "month" | "year" | "all";

export type Bar = { label: string; ms: number; today?: boolean };
export type TopSong = { track: Track; plays: number; ms: number };
export type TopArtist = { name: string; ms: number; plays: number; artwork?: string | null };

export type Summary = {
  ms: number;
  plays: number;
  songs: number;
  artists: number;
  /** listening days in the period */
  activeDays: number;
  /** vs the same length of time just before (null when there's nothing to compare) */
  change: number | null;
  bars: Bar[];
  topSongs: TopSong[];
  topArtists: TopArtist[];
  /** 0–23, the hour with the most listening */
  peakHour: number | null;
  /** days in a row with music, up to today */
  streak: number;
  /** the single biggest day in the period */
  bestDay: { date: Date; ms: number } | null;
};

export const dayKey = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
const addDays = (d: Date, n: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** The days a period covers, oldest first. */
function periodDays(p: Period, now: Date, data: StatsFile): Date[] {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  let start: Date;
  if (p === "week") start = addDays(today, -6);
  else if (p === "month") start = addDays(today, -29);
  else if (p === "year") start = new Date(now.getFullYear(), 0, 1);
  else {
    const keys = Object.keys(data.days).sort();
    const first = keys[0] ?? data.since ?? dayKey(today);
    const [y, m, d] = first.split("-").map(Number);
    start = new Date(y, m - 1, d);
  }
  const out: Date[] = [];
  for (let d = start; d <= today; d = addDays(d, 1)) out.push(d);
  return out;
}

/** "Drake, Wizkid & Kyla" → "Drake". */
export const mainArtist = (a: string) => a.split(/,|&| feat\.? | ft\.? | x /i)[0].trim() || a;

export function summarize(data: StatsFile, p: Period, now = new Date()): Summary {
  const days = periodDays(p, now, data);
  const todayKey = dayKey(now);
  const songMs = new Map<string, StatsSong>();
  const hours = Array<number>(24).fill(0);
  let ms = 0;
  let plays = 0;
  let activeDays = 0;
  let bestDay: Summary["bestDay"] = null;
  for (const d of days) {
    const day = data.days[dayKey(d)];
    if (!day) continue;
    ms += day.ms;
    if (day.ms > 60_000) activeDays++;
    if (!bestDay || day.ms > bestDay.ms) bestDay = { date: d, ms: day.ms };
    day.hours?.forEach((h, i) => (hours[i] += h));
    for (const [id, s] of Object.entries(day.songs)) {
      const cur = songMs.get(id) ?? { ms: 0, plays: 0 };
      cur.ms += s.ms;
      cur.plays += s.plays;
      songMs.set(id, cur);
      plays += s.plays;
    }
  }

  // the same stretch of time just before, for "up 20%"
  let change: number | null = null;
  if (p !== "all") {
    const len = days.length;
    let before = 0;
    for (let i = 1; i <= len; i++) before += data.days[dayKey(addDays(days[0], -i))]?.ms ?? 0;
    if (before > 60_000) change = (ms - before) / before;
  }

  // bars: days for week/month, months for year/all
  let bars: Bar[];
  if (p === "week" || p === "month") {
    bars = days.map((d) => ({ label: p === "week" ? WEEKDAYS[d.getDay()] : String(d.getDate()), ms: data.days[dayKey(d)]?.ms ?? 0, today: dayKey(d) === todayKey }));
  } else {
    const byMonth = new Map<string, number>();
    for (const d of days) {
      const k = `${d.getFullYear()}-${d.getMonth()}`;
      byMonth.set(k, (byMonth.get(k) ?? 0) + (data.days[dayKey(d)]?.ms ?? 0));
    }
    const keys = [...byMonth.keys()].slice(-12);
    bars = keys.map((k) => {
      const [y, m] = k.split("-").map(Number);
      return { label: MONTHS[m], ms: byMonth.get(k) ?? 0, today: y === now.getFullYear() && m === now.getMonth() };
    });
  }

  const meta = (id: string): Track => {
    const t = data.tracks[id];
    return { videoId: id, title: t?.title ?? "Unknown song", artist: t?.artist ?? "", album: t?.album ?? null, artwork: t?.artwork ?? null } as Track;
  };
  const topSongs = [...songMs.entries()]
    .sort((a, b) => b[1].plays - a[1].plays || b[1].ms - a[1].ms)
    .slice(0, 10)
    .map(([id, s]) => ({ track: meta(id), plays: s.plays, ms: s.ms }));

  const artists = new Map<string, TopArtist>();
  for (const [id, s] of songMs) {
    const t = data.tracks[id];
    if (!t?.artist) continue;
    const name = mainArtist(t.artist);
    const a = artists.get(name) ?? { name, ms: 0, plays: 0, artwork: t.artwork };
    a.ms += s.ms;
    a.plays += s.plays;
    artists.set(name, a);
  }
  const topArtists = [...artists.values()].sort((a, b) => b.ms - a.ms).slice(0, 10);

  const peak = hours.reduce((best, v, i) => (v > hours[best] ? i : best), 0);

  // streak: days in a row with music, counting back from today (or yesterday, if today hasn't started)
  let streak = 0;
  let d = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  if ((data.days[dayKey(d)]?.ms ?? 0) < 60_000) d = addDays(d, -1);
  while ((data.days[dayKey(d)]?.ms ?? 0) >= 60_000) {
    streak++;
    d = addDays(d, -1);
  }

  return {
    ms,
    plays,
    songs: songMs.size,
    artists: artists.size,
    activeDays,
    change,
    bars,
    topSongs,
    topArtists,
    peakHour: hours[peak] > 0 ? peak : null,
    streak,
    bestDay: bestDay && bestDay.ms > 0 ? bestDay : null,
  };
}

/** 754 min → "12 h 34 min". */
export function fmtListen(ms: number): string {
  const min = Math.round(ms / 60_000);
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `${h} h ${m} min` : `${h} h`;
}

/** 23 → "11 pm"; and a name for that time of day. */
export function hourLabel(h: number): { time: string; kind: string } {
  const time = h === 0 ? "midnight" : h === 12 ? "noon" : `${h % 12} ${h < 12 ? "am" : "pm"}`;
  const kind = h >= 22 || h < 5 ? "Night owl" : h < 9 ? "Early riser" : h < 12 ? "Morning listener" : h < 17 ? "Afternoon listener" : "Evening listener";
  return { time, kind };
}

/** A believable sample for testing the screens on a computer (the phone uses real data). */
export function sampleStats(): StatsFile {
  const songs: [string, string, string][] = [
    ["s1", "Blinding Lights", "The Weeknd"],
    ["s2", "Sweater Weather", "The Neighbourhood"],
    ["s3", "Do I Wanna Know?", "Arctic Monkeys"],
    ["s4", "Starboy", "The Weeknd, Daft Punk"],
    ["s5", "Circles", "Post Malone"],
    ["s6", "Another Love", "Tom Odell"],
    ["s7", "SICKO MODE", "Travis Scott"],
    ["s8", "Pink + White", "Frank Ocean"],
  ];
  const tracks: StatsFile["tracks"] = {};
  songs.forEach(([id, title, artist]) => (tracks[id] = { title, artist }));
  const days: StatsFile["days"] = {};
  const now = new Date();
  for (let i = 0; i < 70; i++) {
    const d = addDays(now, -i);
    if ((i * 7) % 5 === 3) continue;
    const day: StatsDay = { ms: 0, hours: Array(24).fill(0), songs: {} };
    songs.forEach(([id], j) => {
      const plays = (i + j * 3) % (j + 2);
      if (!plays) return;
      const ms = plays * 200_000;
      day.songs[id] = { ms, plays };
      day.ms += ms;
      day.hours[(20 + j + i) % 24] += ms;
    });
    days[dayKey(d)] = day;
  }
  return { days, tracks, since: dayKey(addDays(now, -70)) };
}
