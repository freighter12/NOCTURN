// Timing model for synced lyrics, kept free of React so it can be tested.
//
// Lines become display items with a start and end. Long stretches without
// singing (an intro, an instrumental break, or a line timestamped with no
// text) become "gap" items shown as three dots that fill up towards the next
// line, so the view never sits frozen on a finished line.
//
// Word timing: real per-word timestamps are used when the lyrics have them
// (enhanced LRC). Otherwise each line's words are *estimated*: the line's
// time is shared out by syllables, with short pauses after commas and full
// stops, and the singing is assumed to take at most ~0.32 s per syllable so
// a line followed by a long silence doesn't stretch its last word out.

export interface LyricItem {
  kind: "line" | "gap";
  start: number;
  end: number;
  text: string;
  /** Word timings (start ms, word) — real, or estimated when `estimated`. */
  words: [number, string][];
  /** The words' timings are an estimate from syllables. */
  estimated?: boolean;
  /** When the last word is done (estimated lines). */
  sungEnd?: number;
}

/** A line shorter than this to the next one never gets a gap after it. */
const GAP_MIN = 9000;
const INTRO_MIN = 4000;
/** Typical time per sung syllable, and the pauses punctuation implies. */
const MS_PER_SYLLABLE = 320;
const COMMA_PAUSE = 0.6; // in syllables
const STOP_PAUSE = 1.0;

/** Rough singing time for a line without word timings (used to find breaks). */
function estimate(text: string): number {
  return Math.min(7000, Math.max(2500, text.length * 110));
}

/**
 * Syllables in a word (English heuristic, fine for other Latin-script
 * languages): vowel groups, minus a silent final "e". Never less than 1.
 * Words without Latin letters (numbers, CJK…) count by length.
 */
export function syllables(word: string): number {
  const w = word.toLowerCase().normalize("NFKD").replace(/[̀-ͯ]/g, "").replace(/[^a-z]/g, "");
  if (!w) {
    const chars = [...word.replace(/[\s\p{P}]/gu, "")].length;
    return Math.max(1, Math.ceil(chars / 2));
  }
  let groups = (w.match(/[aeiouy]+/g) ?? []).length;
  const spokenE = /é$/.test(word.toLowerCase().replace(/[^\p{L}]/gu, "")); // "café"
  if (w.length > 2 && w.endsWith("e") && !spokenE && !w.endsWith("le") && groups > 1) groups--; // "love", "time" — but "little"
  if (/[^aeiouy]ed$/.test(w) && !/[td]ed$/.test(w) && groups > 1) groups--; // "loved", but "wanted"
  return Math.max(1, groups);
}

/** How long the pause after a word is, in syllables. */
function pauseAfter(word: string): number {
  if (/[.!?…]["')\]]*\s*$/.test(word)) return STOP_PAUSE;
  if (/[,;:—–-]["')\]]*\s*$/.test(word)) return COMMA_PAUSE;
  return 0;
}

/**
 * Estimate word start times across a line that starts at `start` and has
 * `available` ms until the next line. Returns the words (with their trailing
 * spaces kept, so they render as written) and when the singing ends.
 */
export function estimateWords(text: string, start: number, available: number): { words: [number, string][]; sungEnd: number } {
  const tokens = text.match(/\S+\s*/g) ?? [];
  if (!tokens.length) return { words: [], sungEnd: start };
  const syl = tokens.map((t) => syllables(t));
  const pauses = tokens.map((t, i) => (i < tokens.length - 1 ? pauseAfter(t) : 0));
  const weight = syl.map((s, i) => s + pauses[i]);
  const total = weight.reduce((a, b) => a + b, 0);
  // Singing takes at most ~0.32 s a syllable, and leaves a little air before the next line.
  const natural = total * MS_PER_SYLLABLE;
  const span = Math.max(Math.min(available, 600), Math.min(available * 0.92, natural));
  const words: [number, string][] = [];
  let acc = 0;
  tokens.forEach((t, i) => {
    words.push([Math.round(start + (span * acc) / total), t]);
    acc += weight[i];
  });
  return { words, sungEnd: Math.round(start + span) };
}

export function buildItems(synced: [number, string][], words: [number, string][][], durationMs: number, estimateWordTimes = true): LyricItem[] {
  const out: LyricItem[] = [];
  const lines = synced.map(([t, text], i) => ({ t, text: text.trim(), words: words[i] ?? [] }));
  if (!lines.length) return out;
  if (lines[0].t >= INTRO_MIN) out.push({ kind: "gap", start: 0, end: lines[0].t, text: "", words: [] });
  const line = (start: number, end: number, l: { t: number; text: string; words: [number, string][] }): LyricItem => {
    if (l.words.length || !estimateWordTimes) return { kind: "line", start, end, text: l.text, words: l.words };
    const e = estimateWords(l.text, start, end - start);
    return { kind: "line", start, end, text: l.text, words: e.words, estimated: true, sungEnd: e.sungEnd };
  };
  lines.forEach((l, i) => {
    const next = lines[i + 1]?.t ?? Math.max(l.t + 4000, durationMs || l.t + 4000);
    if (!l.text) {
      // "[01:02.00]" with no text marks an instrumental part.
      if (next - l.t >= 2000) out.push({ kind: "gap", start: l.t, end: next, text: "", words: [] });
      return;
    }
    const sung = l.words.length ? l.words[l.words.length - 1][0] - l.t + 900 : estimate(l.text);
    const isLast = i === lines.length - 1;
    if (!isLast && next - l.t > GAP_MIN && next - (l.t + sung) > 4000) {
      out.push(line(l.t, l.t + sung, l));
      out.push({ kind: "gap", start: l.t + sung, end: next, text: "", words: [] });
    } else {
      out.push(line(l.t, isLast ? Math.min(next, l.t + Math.max(sung, 4000)) : next, l));
    }
  });
  return out;
}

/** Index of the item playing at `t` (last one that has started), or -1 before the first. */
export function activeIndex(items: LyricItem[], t: number): number {
  let lo = 0;
  let hi = items.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (items[mid].start <= t) {
      ans = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return ans;
}

/** 0–1 through an item. */
export function progress(item: LyricItem, t: number): number {
  const span = Math.max(1, item.end - item.start);
  return Math.min(1, Math.max(0, (t - item.start) / span));
}

/** 0–1 for each word of a line (a word fills until the next word starts). */
export function wordProgress(item: LyricItem, t: number): number[] {
  return item.words.map(([wt], j) => {
    const next = item.words[j + 1]?.[0] ?? (item.sungEnd ?? Math.min(item.end, wt + 900));
    return Math.min(1, Math.max(0, (t - wt) / Math.max(1, next - wt)));
  });
}

/**
 * The lyrics clock: where the song is as you *hear* it, plus your nudge.
 * The speakers play `latency` ms behind the player; a positive offset shows
 * lyrics earlier.
 */
export function lyricTime(playerMs: number, latencyMs: number, offsetMs: number): number {
  return playerMs - latencyMs + offsetMs;
}
