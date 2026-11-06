import { describe, expect, it } from "vitest";
import { PAGE_IDS, SECTION_IDS } from "./nav";
import { DEFAULT_PREFS, isHexColour, sanitize } from "./prefs";

const clean = (raw: unknown) => sanitize(raw, PAGE_IDS, SECTION_IDS);

describe("sanitize", () => {
  it("returns defaults for nothing or garbage", () => {
    expect(clean(null)).toEqual(DEFAULT_PREFS);
    expect(clean("nope")).toEqual(DEFAULT_PREFS);
    expect(clean([1, 2])).toEqual(DEFAULT_PREFS);
  });

  it("keeps valid values and fixes invalid ones field by field", () => {
    const p = clean({
      appearance: { theme: "dusk", accent: "#ff00aa", radius: 99, transparency: -1, blur: "lots" },
      accessibility: { fontScale: 1.2, uiScale: 9 },
      music: { playerMode: "mini", volume: 2 },
    });
    expect(p.appearance.theme).toBe("dusk");
    expect(p.appearance.accent).toBe("#ff00aa");
    expect(p.appearance.radius).toBe(2);
    expect(p.appearance.transparency).toBe(0);
    expect(p.appearance.blur).toBe(DEFAULT_PREFS.appearance.blur);
    expect(p.accessibility.fontScale).toBe(1.2);
    expect(p.accessibility.uiScale).toBe(1.5);
    expect(p.music.playerMode).toBe("mini");
    expect(p.music.volume).toBe(1);
  });

  it("never hides System or Settings and keeps every section reachable", () => {
    const p = clean({ navigation: { hidden: ["system", "settings", "music", "made-up"], order: ["settings", "home", "bogus"] } });
    expect(p.navigation.hidden).toEqual(["music"]);
    expect(p.navigation.order[0]).toBe("settings");
    expect(p.navigation.order[1]).toBe("home");
    expect([...p.navigation.order].sort()).toEqual([...SECTION_IDS].sort());
  });

  it("rejects unknown pages for start page and pins", () => {
    const p = clean({ navigation: { startPage: "deleted-page", pinned: ["tweak", "nope", "tweak"] } });
    expect(p.navigation.startPage).toBe("home");
    expect(p.navigation.pinned).toEqual(["tweak"]);
  });

  it("drops unknown or duplicate widgets", () => {
    const p = clean({ home: { widgets: [{ id: "a", type: "vitals", size: "l" }, { id: "a", type: "health", size: "m" }, { id: "b", type: "crypto-miner", size: "s" }, { id: "c", type: "storage", size: "huge" }] } });
    expect(p.home.widgets).toEqual([{ id: "a", type: "vitals", size: "l" }, { id: "c", type: "storage", size: "m" }]);
  });

  it("accepts only hex colours", () => {
    expect(isHexColour("#abc")).toBe(true);
    expect(isHexColour("#a1b2c3")).toBe(true);
    expect(isHexColour("red")).toBe(false);
    expect(isHexColour("#12345")).toBe(false);
    expect(isHexColour("url(javascript:x)")).toBe(false);
  });
});
