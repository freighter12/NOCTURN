// Pure helpers for the playback engine: volume levelling and transitions.
// Kept free of React and the audio element so they can be unit-tested.

import type { Track } from "../lib/api";

/** Everything is levelled to this loudness (what most streaming services use). */
export const TARGET_LUFS = -14;
/** Never boost or cut more than this. */
const MAX_BOOST_DB = 8;
const MAX_CUT_DB = -14;
/** Leave this much room under full scale when boosting quiet songs. */
const HEADROOM_DB = 0.5;
/** Until a song has been measured, assume a typical modern master. */
export const DEFAULT_GAIN_DB = -4;

/** Gain (dB) that brings a song with this loudness and peak to the target. */
export function gainFor(lufs: number, peak: number | null | undefined): number {
  let g = TARGET_LUFS - lufs;
  g = Math.min(MAX_BOOST_DB, Math.max(MAX_CUT_DB, g));
  if (g > 0 && peak && peak > 0) {
    // Only boost as far as the loudest sample allows, so nothing clips.
    const room = -20 * Math.log10(peak) - HEADROOM_DB;
    g = Math.min(g, Math.max(0, room));
  }
  return Math.round(g * 10) / 10;
}

/** Median gain of the measured songs: the best guess for one not measured yet. */
export function typicalGain(tracks: Pick<Track, "loudness" | "peak">[]): number {
  const gs = tracks.filter((t) => t.loudness != null).map((t) => gainFor(t.loudness!, t.peak)).sort((a, b) => a - b);
  if (gs.length < 5) return DEFAULT_GAIN_DB;
  const m = gs.length >> 1;
  return gs.length % 2 ? gs[m] : (gs[m - 1] + gs[m]) / 2;
}

export const dbToGain = (db: number) => Math.pow(10, db / 20);

/** The levelling gain (linear) for a track, or 1 when levelling is off. */
export function levelFor(t: Pick<Track, "loudness" | "peak"> | null | undefined, on: boolean, fallbackDb: number): number {
  if (!on || !t) return 1;
  return dbToGain(t.loudness != null ? gainFor(t.loudness, t.peak) : fallbackDb);
}

/** Songs that run straight into each other on an album shouldn't be crossfaded. */
export function sameAlbumRun(a: Track | null | undefined, b: Track | null | undefined): boolean {
  if (!a || !b || !a.album || a.album !== b.album || a.album_artist !== b.album_artist) return false;
  if (a.track_no == null || b.track_no == null) return false;
  return (a.disc_no ?? 1) === (b.disc_no ?? 1) ? b.track_no === a.track_no + 1 : (b.disc_no ?? 1) === (a.disc_no ?? 1) + 1 && b.track_no === 1;
}

/**
 * How many seconds the next song should overlap this one: the crossfade
 * length, or 0 (gapless / plain) for album runs and songs too short to fade.
 */
export function overlapFor(a: Track | null | undefined, b: Track | null | undefined, crossfade: number, keepAlbums: boolean): number {
  if (crossfade <= 0 || !a || !b) return 0;
  if (keepAlbums && sameAlbumRun(a, b)) return 0;
  // A fade can't take more than a third of either song.
  const cap = Math.min(a.duration_ms, b.duration_ms) / 1000 / 3;
  return cap < 1 ? 0 : Math.min(crossfade, cap);
}

/** Equal-power fade curve (no dip in loudness halfway through). */
export function fadeCurve(points: number, up: boolean, from = 1): Float32Array {
  const c = new Float32Array(Math.max(2, points));
  for (let i = 0; i < c.length; i++) {
    const x = i / (c.length - 1);
    c[i] = from * (up ? Math.sin((x * Math.PI) / 2) : Math.cos((x * Math.PI) / 2));
  }
  return c;
}
