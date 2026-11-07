import { describe, expect, it } from "vitest";
import { litPath, moonPhase, seeded, skyTime } from "./sky";

describe("moon phase", () => {
  it("matches known moons", () => {
    // Full moon 2024-04-23 23:49 UTC; new moon 2024-04-08 18:21 UTC (the eclipse).
    const full = moonPhase(new Date(Date.UTC(2024, 3, 23, 23, 49)));
    expect(full.name).toBe("Full moon");
    expect(full.illumination).toBeGreaterThan(0.98);
    const nw = moonPhase(new Date(Date.UTC(2024, 3, 8, 18, 21)));
    expect(nw.name).toBe("New moon");
    expect(nw.illumination).toBeLessThan(0.02);
    // First quarter 2024-04-15 19:13 UTC: about half lit, waxing.
    const fq = moonPhase(new Date(Date.UTC(2024, 3, 15, 19, 13)));
    expect(fq.name).toBe("First quarter");
    expect(fq.waxing).toBe(true);
    // The average-cycle formula can be up to ~half a day out (the real orbit is elliptical): a few % of lit area.
    expect(fq.illumination).toBeGreaterThan(0.4);
    expect(fq.illumination).toBeLessThan(0.6);
  });

  it("counts down to the full moon", () => {
    const p = moonPhase(new Date(Date.UTC(2024, 3, 16, 0, 0)));
    expect(p.daysToFull).toBeGreaterThan(7);
    expect(p.daysToFull).toBeLessThan(8.5);
  });
});

describe("lit path", () => {
  it("is empty at new moon and a full circle-ish at full moon", () => {
    expect(litPath(0, 50, 50, 40)).toBe("");
    const full = litPath(0.5, 50, 50, 40);
    expect(full).toContain("A 40.00 40"); // terminator as wide as the moon
  });
  it("lights the right side when waxing, the left when waning", () => {
    expect(litPath(0.2, 50, 50, 40)).toMatch(/A 40 40 0 0 1/); // right limb
    expect(litPath(0.8, 50, 50, 40)).toMatch(/A 40 40 0 0 0/); // left limb
  });
});

describe("sky", () => {
  it("picks the time of day", () => {
    const at = (h: number) => skyTime(new Date(2024, 0, 1, h, 0));
    expect([at(2), at(6), at(12), at(19), at(22)]).toEqual(["night", "dawn", "day", "dusk", "night"]);
  });
  it("seeded random is repeatable", () => {
    const a = seeded(7);
    const b = seeded(7);
    expect([a(), a(), a()]).toEqual([b(), b(), b()]);
  });
});
