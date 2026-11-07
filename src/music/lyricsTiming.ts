// Timing model for synced lyrics, kept free of React so it can be tested.
//
// Lines become display items with a start and end. Long stretches without
// singing (an intro, an instrumental break, or a line timestamped with no
// text) become "gap" items shown as three dots that fill up towards the next
// line, so the view never sits frozen on a finished line.

export interface LyricItem {
  kind: "line" | "gap";
  start: number;
  end: number;
  text: string;
  /** Word timings (start ms, word) when the source has them. */
  words: [number, string][];
}

/** A line shorter than this to the next one never gets a gap after it. */
const GAP_MIN = 9000;
const INTRO_MIN = 4000;

/** Rough singing time for a line without word timings. */
function estimate(text: string): number {
  return Math.min(7000, Math.max(2500, text.length * 110));
}

export function buildItems(synced: [number, string][], words: [number, string][][], durationMs: number): LyricItem[] {
  const out: LyricItem[] = [];
  const lines = synced.map(([t, text], i) => ({ t, text: text.trim(), words: words[i] ?? [] }));
  if (!lines.length) return out;
  if (lines[0].t >= INTRO_MIN) out.push({ kind: "gap", start: 0, end: lines[0].t, text: "", words: [] });
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
      out.push({ kind: "line", start: l.t, end: l.t + sung, text: l.text, words: l.words });
      out.push({ kind: "gap", start: l.t + sung, end: next, text: "", words: [] });
    } else {
      out.push({ kind: "line", start: l.t, end: isLast ? Math.min(next, l.t + Math.max(sung, 4000)) : next, text: l.text, words: l.words });
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
    const next = item.words[j + 1]?.[0] ?? Math.min(item.end, wt + 900);
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
