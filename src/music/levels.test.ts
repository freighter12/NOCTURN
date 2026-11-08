import { describe, expect, it } from "vitest";
import type { Track } from "../lib/api";
import { DEFAULT_GAIN_DB, dbToGain, fadeCurve, gainFor, levelFor, overlapFor, sameAlbumRun, typicalGain } from "./levels";

const t = (o: Partial<Track>): Track => ({
  id: 1, source: "local", title: "x", artist: "a", album: "Album", album_artist: "a", track_no: 1, disc_no: 1, year: null, genre: "",
  duration_ms: 200_000, has_art: false, liked: false, added_at: "", plays: 0, last_played: null, loudness: null, peak: null, ...o,
});

describe("levelling", () => {
  it("turns loud songs down and quiet songs up to the target", () => {
    expect(gainFor(-8, 1)).toBe(-6);
    expect(gainFor(-20, 0.3)).toBe(6);
  });
  it("never boosts past the peak", () => {
    // Peak 0.8 ≈ -1.9 dBFS → at most +1.4 dB.
    expect(gainFor(-20, 0.8)).toBeCloseTo(1.4, 1);
    expect(gainFor(-20, 1.2)).toBe(0);
  });
  it("caps extreme corrections", () => {
    expect(gainFor(-50, 0.01)).toBe(8);
    expect(gainFor(10, 1)).toBe(-14);
  });
  it("guesses unmeasured songs from the library", () => {
    expect(typicalGain([])).toBe(DEFAULT_GAIN_DB);
    const lib = [-8, -9, -10, -11, -12].map((l) => ({ loudness: l, peak: 1 }));
    expect(typicalGain(lib)).toBe(-4);
    expect(levelFor(t({}), true, -4)).toBeCloseTo(dbToGain(-4));
    expect(levelFor(t({ loudness: -8, peak: 1 }), false, -4)).toBe(1);
  });
});

describe("transitions", () => {
  it("knows album runs, across discs too", () => {
    expect(sameAlbumRun(t({ track_no: 3 }), t({ track_no: 4 }))).toBe(true);
    expect(sameAlbumRun(t({ track_no: 3 }), t({ track_no: 5 }))).toBe(false);
    expect(sameAlbumRun(t({ track_no: 12, disc_no: 1 }), t({ track_no: 1, disc_no: 2 }))).toBe(true);
    expect(sameAlbumRun(t({ track_no: 3 }), t({ track_no: 4, album: "Other" }))).toBe(false);
  });
  it("crossfades except album runs and very short songs", () => {
    expect(overlapFor(t({ track_no: 1 }), t({ track_no: 9 }), 6, true)).toBe(6);
    expect(overlapFor(t({ track_no: 1 }), t({ track_no: 2 }), 6, true)).toBe(0);
    expect(overlapFor(t({ track_no: 1 }), t({ track_no: 2 }), 6, false)).toBe(6);
    expect(overlapFor(t({}), t({ duration_ms: 9_000, track_no: 7 }), 6, true)).toBe(3);
    expect(overlapFor(t({}), t({ track_no: 7 }), 0, true)).toBe(0);
  });
  it("fades with equal power", () => {
    const up = fadeCurve(101, true);
    const down = fadeCurve(101, false);
    expect(up[0]).toBe(0);
    expect(down[0]).toBe(1);
    // Halfway, the combined power is still 1.
    expect(up[50] ** 2 + down[50] ** 2).toBeCloseTo(1, 5);
  });
});
