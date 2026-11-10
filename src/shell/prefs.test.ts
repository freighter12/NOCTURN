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
    // A new section lands after its default neighbour, not at the end.
    const q = clean({ navigation: { order: ["home", "library", "music", "workspaces", "ai", "gaming", "system", "studio", "settings"] } });
    expect(q.navigation.order.indexOf("winddown")).toBe(q.navigation.order.indexOf("workspaces") + 1);
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

import { OFFICIAL_THEMES, accentProblem, contrast, readabilityProblem } from "./theme";
import { sanitizeTheme } from "./prefs";

describe("themes", () => {
  it("every official theme is readable", () => {
    for (const t of OFFICIAL_THEMES) expect(readabilityProblem(t.tokens), t.id).toBeNull();
  });

  it("contrast maths matches WCAG", () => {
    expect(contrast("#000000", "#ffffff")).toBeCloseTo(21, 0);
    expect(contrast("#777777", "#777777")).toBeCloseTo(1, 5);
  });

  it("rejects unreadable or malformed custom themes", () => {
    const base = OFFICIAL_THEMES[0];
    const ok = sanitizeTheme({ ...base, id: "custom-mine", name: "Mine" });
    expect(ok?.name).toBe("Mine");
    expect(sanitizeTheme({ ...base, id: "custom-grey", tokens: { ...base.tokens, ink0: base.tokens.bg2 } })).toBeNull();
    expect(sanitizeTheme({ ...base, id: "nocturn" })).toBeNull();
    expect(sanitizeTheme({ ...base, id: "custom-x", tokens: { ...base.tokens, bg0: "red" } })).toBeNull();
    expect(sanitizeTheme({ ...base, id: "../../etc" })).toBeNull();
  });

  it("flags accents that disappear on the panel", () => {
    expect(accentProblem("#13141a", OFFICIAL_THEMES[0].tokens)).not.toBeNull();
    expect(accentProblem("#9fc3ff", OFFICIAL_THEMES[0].tokens)).toBeNull();
  });
});

import { sanitizeWorkspace } from "./prefs";

describe("workspaces", () => {
  it("keeps valid workspaces and drops broken ones", () => {
    const p = clean({
      workspaces: {
        active: "ws-game",
        startWith: "ws-missing",
        list: [
          { id: "ws-game", name: "Gaming", glyph: "🎮x", accent: "#9fc3ff", background: "glow", apps: ["a", "b"], launch: ["b", "z"], widgets: [{ id: "w1", type: "vitals", size: "l" }] },
          { id: "../../evil", name: "x" },
          { id: "ws-game", name: "Duplicate" },
        ],
      },
    });
    expect(p.workspaces.list).toHaveLength(1);
    const ws = p.workspaces.list[0];
    // Two characters max, counting an emoji as one.
    expect(ws.glyph).toBe("🎮x");
    expect(ws.launch).toEqual(["b"]);
    expect(ws.widgets).toEqual([{ id: "w1", type: "vitals", size: "l" }]);
    expect(p.workspaces.active).toBe("ws-game");
    expect(p.workspaces.startWith).toBeNull();
  });

  it("an active workspace that no longer exists falls back to none", () => {
    expect(clean({ workspaces: { active: "ws-gone", list: [] } }).workspaces.active).toBeNull();
  });

  it("rejects bad colours and backgrounds", () => {
    const ws = sanitizeWorkspace({ id: "ws-a", accent: "javascript:x", background: "lava" });
    expect(ws?.accent).toBeNull();
    expect(ws?.background).toBeNull();
    expect(ws?.widgets).toBeNull();
  });
});

describe("intro", () => {
  it("defaults to the full animation and accepts only known modes", () => {
    expect(clean({}).appearance.intro).toBe("full");
    expect(clean({ appearance: { intro: "short" } }).appearance.intro).toBe("short");
    expect(clean({ appearance: { intro: "off" } }).appearance.intro).toBe("off");
    expect(clean({ appearance: { intro: "fireworks" } }).appearance.intro).toBe("full");
  });
});

import { greetingTitle } from "./greeting";
import { DEFAULT_GREETING } from "./prefs";

describe("greeting", () => {
  const evening = new Date(2026, 10, 10, 20, 0);
  it("defaults to the time of day, with the name if set", () => {
    expect(greetingTitle(DEFAULT_GREETING, evening)).toBe("Good evening");
    expect(greetingTitle({ ...DEFAULT_GREETING, name: "Sam" }, evening)).toBe("Good evening, Sam");
  });
  it("fills placeholders and drops a dangling comma", () => {
    expect(greetingTitle({ ...DEFAULT_GREETING, text: "{greeting}, {name}!", name: "Sam" }, evening)).toBe("Good evening, Sam!");
    expect(greetingTitle({ ...DEFAULT_GREETING, text: "{greeting}, {name}!" }, evening)).toBe("Good evening!");
    expect(greetingTitle({ ...DEFAULT_GREETING, text: "Welcome back" }, evening)).toBe("Welcome back");
  });
  it("sanitises stored greetings", () => {
    const p = clean({ home: { greeting: { name: "  Sam\u0007 ", sky: "purple", showClock: "yes" } } });
    expect(p.home.greeting.name).toBe("Sam");
    expect(p.home.greeting.sky).toBe("auto");
    expect(p.home.greeting.showClock).toBe(false);
  });
});
