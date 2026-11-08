import { describe as group, expect, it } from "vitest";
import type { MusicStats, Track } from "../lib/api";
import { builtins, describe, evaluate, hourScores, sanitizeRules, seededShuffle, timeOfDay, type SmartRules } from "./smart";

const NOW = Date.parse("2026-11-08T21:00:00Z");
const daysAgo = (d: number) => new Date(NOW - d * 86_400_000).toISOString();
let n = 0;
const t = (o: Partial<Track>): Track => ({
  id: ++n, source: "local", title: `Song ${n}`, artist: "A", album: "Al", album_artist: "A", track_no: null, disc_no: null, year: 2020, genre: "",
  duration_ms: 200_000, has_art: false, liked: false, added_at: daysAgo(100), plays: 0, last_played: null, loudness: null, peak: null, ...o,
});

group("rules", () => {
  const lib = [
    t({ genre: "Jazz", plays: 10, liked: true }),
    t({ genre: "Smooth jazz", plays: 2 }),
    t({ genre: "Rock", plays: 50, added_at: daysAgo(3) }),
    t({ genre: "Rock", plays: 0, last_played: null }),
  ];
  const ids = lib.map((x) => x.id);
  it("match all / any", () => {
    const r: SmartRules = { match: "all", rules: [{ field: "genre", op: "contains", value: "jazz" }, { field: "liked", op: "is", value: true }], sort: "plays", limit: null };
    expect(evaluate(r, lib, NOW)).toEqual([ids[0]]);
    expect(evaluate({ ...r, match: "any" }, lib, NOW)).toEqual([ids[0], ids[1]]);
  });
  it("numbers, dates, never played, sort and limit", () => {
    expect(evaluate({ match: "all", rules: [{ field: "plays", op: "gt", value: 5 }], sort: "plays", limit: null }, lib, NOW)).toEqual([ids[2], ids[0]]);
    expect(evaluate({ match: "all", rules: [{ field: "added", op: "within", value: 7 }], sort: "plays", limit: null }, lib, NOW)).toEqual([ids[2]]);
    expect(evaluate({ match: "all", rules: [{ field: "played", op: "never", value: 0 }], sort: "plays", limit: 2 }, lib, NOW)).toHaveLength(2);
    expect(evaluate({ match: "all", rules: [], sort: "plays", limit: 1 }, lib, NOW)).toEqual([ids[2]]);
  });
  it("sanitises junk", () => {
    const r = sanitizeRules({ match: "xx", rules: [{ field: "genre", op: "??", value: 3 }, { field: "nope" }, { field: "added", op: "never" }], sort: "bad", limit: -5 });
    expect(r.match).toBe("all");
    expect(r.rules).toEqual([{ field: "genre", op: "contains", value: "" }, { field: "added", op: "within", value: 30 }]);
    expect(r.sort).toBe("plays");
    expect(r.limit).toBe(1);
    expect(sanitizeRules(null).rules).toEqual([]);
  });
  it("describes itself", () => {
    expect(describe({ match: "all", rules: [{ field: "genre", op: "contains", value: "jazz" }, { field: "liked", op: "is", value: true }], sort: "plays", limit: 100 })).toBe('Genre contains "jazz" and liked · most played · up to 100');
  });
});

group("built-in mixes", () => {
  it("names the time of day", () => {
    expect(timeOfDay(8).key).toBe("morning");
    expect(timeOfDay(23).name).toBe("Late-night mix");
    expect(timeOfDay(2).key).toBe("night");
  });
  it("scores plays near the hour, wrapping round midnight", () => {
    const s: MusicStats = { recent: [], by_hour: [[1, 23, 4], [2, 1, 4], [3, 12, 9]] };
    const sc = hourScores(s, 0);
    expect(sc.get(1)).toBeCloseTo(4 * (2 / 3));
    expect(sc.get(2)).toBeCloseTo(4 * (2 / 3));
    expect(sc.has(3)).toBe(false);
  });
  it("shows only mixes with enough songs", () => {
    const lib = Array.from({ length: 8 }, (_, i) => t({ plays: i < 6 ? 8 : 0, liked: i < 2, last_played: daysAgo(90), genre: i < 5 ? "Ambient" : "Pop" }));
    const stats: MusicStats = { recent: lib.slice(0, 6).map((x) => [x.id, 3]), by_hour: lib.slice(0, 6).map((x) => [x.id, 21, 2]) };
    const names = builtins(lib, stats, NOW).map((l) => l.id);
    expect(names).toContain("builtin:repeat");
    expect(names).toContain("builtin:forgotten");
    expect(names).toContain("builtin:winddown");
    expect(names).toContain("builtin:most");
    expect(names).not.toContain("builtin:new");
    expect(names).not.toContain("builtin:unplayed");
  });
  it("daily shuffles are stable for the day", () => {
    const xs = Array.from({ length: 20 }, (_, i) => i);
    expect(seededShuffle(xs, "a")).toEqual(seededShuffle(xs, "a"));
    expect(seededShuffle(xs, "a")).not.toEqual(seededShuffle(xs, "b"));
    expect([...seededShuffle(xs, "a")].sort((a, b) => a - b)).toEqual(xs);
  });
});
