import { describe, expect, it } from "vitest";
import { bytes, bytesParts, duration, pct, rate, until } from "./format";

describe("format", () => {
  it("formats byte sizes with binary units", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(1536)).toBe("1.5 KB");
    expect(bytes(16 * 1024 ** 3)).toBe("16.0 GB");
    expect(bytes(500 * 1024 ** 3)).toBe("500 GB");
    expect(bytes(null)).toBe("—");
    expect(bytesParts(2 * 1024 ** 2)).toEqual(["2.0", "MB"]);
  });

  it("formats bit rates from bytes per second", () => {
    expect(rate(10)).toBe("80 b/s");
    expect(rate(125_000)).toBe("1.0 Mb/s");
    expect(rate(12_500_000)).toBe("100 Mb/s");
  });

  it("formats percentages and durations", () => {
    expect(pct(42.4)).toBe("42%");
    expect(pct(undefined)).toBe("—");
    expect(duration(59)).toBe("0m");
    expect(duration(3 * 3600 + 120)).toBe("3h 2m");
    expect(duration(2 * 86400 + 5 * 3600)).toBe("2d 5h");
  });
});

describe("until", () => {
  it("describes future times and treats the past as now", () => {
    const at = (s: number) => new Date(Date.now() + s * 1000).toISOString();
    expect(until(at(-100))).toBe("now");
    expect(until(at(30 * 60))).toBe("in 30 min");
    expect(until(at(5 * 3600))).toBe("in 5 h");
    expect(until(at(6 * 86400))).toBe("in 6 d");
    expect(until(null)).toBe("—");
  });
});
