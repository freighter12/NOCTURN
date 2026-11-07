import { describe, expect, it } from "vitest";
import { activeIndex, buildItems, lyricTime, progress, wordProgress } from "./lyricsTiming";

describe("buildItems", () => {
  it("adds an intro gap, a gap for long instrumental breaks, and instrumental markers", () => {
    const items = buildItems(
      [
        [8000, "First line"],
        [11000, "Second line"],
        [40000, "After a long break"],
        [44000, ""],
        [52000, "Last"],
      ],
      [],
      60000,
    );
    expect(items.map((i) => `${i.kind}:${i.start}-${i.end}`)).toEqual([
      "gap:0-8000",
      "line:8000-11000",
      "line:11000-13500", // sung for ~2.5 s (estimated) …
      "gap:13500-40000", // … then a break until the next line
      "line:40000-44000",
      "gap:44000-52000", // empty timestamp = instrumental
      "line:52000-56000",
    ]);
  });

  it("no intro gap when singing starts quickly; word timings set the sung length", () => {
    const items = buildItems(
      [
        [1000, "Hey Jude"],
        [30000, "Next"],
      ],
      [[[1000, "Hey "], [1500, "Jude"]], []],
      40000,
    );
    expect(items[0]).toMatchObject({ kind: "line", start: 1000, end: 2400 }); // last word at 1.5 s + 0.9 s
    expect(items[1]).toMatchObject({ kind: "gap", start: 2400, end: 30000 });
  });
});

describe("timing", () => {
  const items = buildItems(
    [
      [1000, "a"],
      [3000, "b"],
      [5000, "c"],
    ],
    [],
    8000,
  );
  it("finds the active item", () => {
    expect(activeIndex(items, 0)).toBe(-1);
    expect(activeIndex(items, 1000)).toBe(0);
    expect(activeIndex(items, 2999)).toBe(0);
    expect(activeIndex(items, 3000)).toBe(1);
    expect(activeIndex(items, 99999)).toBe(2);
  });
  it("reports progress and per-word progress", () => {
    expect(progress(items[0], 2000)).toBe(0.5);
    expect(progress(items[0], 9999)).toBe(1);
    const line = { kind: "line" as const, start: 0, end: 2000, text: "x y", words: [[0, "x "], [1000, "y"]] as [number, string][] };
    expect(wordProgress(line, 500)).toEqual([0.5, 0]);
    // The last word fills over 0.9 s (no next word to aim at).
    const [first, last] = wordProgress(line, 1500);
    expect(first).toBe(1);
    expect(last).toBeCloseTo(500 / 900);
  });
  it("compensates speaker delay and the user's nudge", () => {
    expect(lyricTime(10000, 200, 0)).toBe(9800); // hear it 0.2 s later
    expect(lyricTime(10000, 0, 300)).toBe(10300); // "earlier" nudge
  });
});
