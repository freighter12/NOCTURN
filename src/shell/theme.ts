// Data-driven themes. A theme is a small set of colours; everything else
// (borders, hover tints, chart gridlines, status text) is derived from them,
// so custom themes can't get those wrong. Applying a theme only sets CSS
// variables on :root — switching is instant.

import type { BackgroundStyle, Prefs } from "./prefs";

export interface ThemeTokens {
  /** Window, sidebar, panels, raised, inputs — darkest to lightest on dark themes. */
  bg0: string; bg1: string; bg2: string; bg3: string; bg4: string;
  /** Text: primary, secondary, muted, faint. */
  ink0: string; ink1: string; ink2: string; ink3: string;
  accent: string; accent2: string;
}

export interface Theme {
  id: string;
  name: string;
  description: string;
  kind: "dark" | "light";
  tokens: ThemeTokens;
  /** Animated themes bring a moving page background (still with reduce motion). */
  scene?: BackgroundStyle;
}

export const OFFICIAL_THEMES: Theme[] = [
  { id: "nocturn", name: "Nocturn", description: "Deep charcoal with moonlight silver — the original.", kind: "dark",
    tokens: { bg0: "#08080a", bg1: "#0d0e11", bg2: "#121317", bg3: "#181a1f", bg4: "#20232a", ink0: "#eceef2", ink1: "#b4b8c2", ink2: "#7d828d", ink3: "#555a64", accent: "#dfe2e8", accent2: "#aeb3be" } },
  { id: "eclipse", name: "Eclipse", description: "True black planes for OLED screens.", kind: "dark",
    tokens: { bg0: "#000000", bg1: "#030304", bg2: "#08090b", bg3: "#101114", bg4: "#17191d", ink0: "#f2f3f5", ink1: "#b9bcc4", ink2: "#80848e", ink3: "#575b64", accent: "#e8eaee", accent2: "#a9adb7" } },
  { id: "dusk", name: "Dusk", description: "Night-blue planes with a pale blue moon.", kind: "dark",
    tokens: { bg0: "#070910", bg1: "#0b0e17", bg2: "#10141f", bg3: "#161b28", bg4: "#1e2433", ink0: "#e9edf6", ink1: "#b0b8ca", ink2: "#7a8399", ink3: "#525a6d", accent: "#b9cdf5", accent2: "#8d9fc4" } },
  { id: "ember", name: "Ember", description: "Warm charcoal with a soft amber glow.", kind: "dark",
    tokens: { bg0: "#0a0807", bg1: "#100d0b", bg2: "#16120f", bg3: "#1d1814", bg4: "#26201b", ink0: "#f3eee9", ink1: "#c3b8ad", ink2: "#8c8177", ink3: "#5f564e", accent: "#f0c89a", accent2: "#c2a07b" } },
  { id: "aurora", name: "Aurora", description: "Deep green-black with a mint accent.", kind: "dark",
    tokens: { bg0: "#060908", bg1: "#0a0e0d", bg2: "#0f1513", bg3: "#151c1a", bg4: "#1c2522", ink0: "#e9f2ef", ink1: "#b2c2bd", ink2: "#7b8d88", ink3: "#53625e", accent: "#a6e3cf", accent2: "#80b3a3" } },
  { id: "northern-lights", name: "Northern Lights", description: "Ribbons of green and teal drifting across a polar night.", kind: "dark", scene: "aurora",
    tokens: { bg0: "#04090a", bg1: "#070d0f", bg2: "#0b1416", bg3: "#111c1f", bg4: "#182629", ink0: "#e8f5f2", ink1: "#afc7c1", ink2: "#7a948e", ink3: "#526662", accent: "#7ee8c7", accent2: "#5fb8a0" } },
  { id: "nebula", name: "Nebula", description: "Slowly turning violet and rose clouds of a distant nebula.", kind: "dark", scene: "nebula",
    tokens: { bg0: "#08060e", bg1: "#0c0914", bg2: "#120e1c", bg3: "#191426", bg4: "#221b33", ink0: "#f0ebf8", ink1: "#bdb3cf", ink2: "#887e9c", ink3: "#5d5570", accent: "#d6b0ff", accent2: "#a98bcc" } },
  { id: "synthwave", name: "Synthwave", description: "Neon magenta over a gliding retro horizon.", kind: "dark", scene: "waves",
    tokens: { bg0: "#0a0612", bg1: "#0f0919", bg2: "#150d22", bg3: "#1d132e", bg4: "#271a3c", ink0: "#f6eefc", ink1: "#c6b5d6", ink2: "#9282a6", ink3: "#655778", accent: "#ff86dc", accent2: "#c76fb0" } },
  { id: "starfall", name: "Starfall", description: "Deep navy sky with drifting, twinkling stars.", kind: "dark", scene: "starfield",
    tokens: { bg0: "#04060d", bg1: "#070a14", bg2: "#0b0f1c", bg3: "#111727", bg4: "#182033", ink0: "#eaeefa", ink1: "#b1b9d0", ink2: "#7c859e", ink3: "#545c73", accent: "#c4d4ff", accent2: "#97a7d1" } },
  { id: "fireflies", name: "Fireflies", description: "A warm summer night with glowing fireflies rising.", kind: "dark", scene: "fireflies",
    tokens: { bg0: "#070806", bg1: "#0b0d09", bg2: "#11140e", bg3: "#181c14", bg4: "#21261b", ink0: "#f2f2e8", ink1: "#c1c2ae", ink2: "#8d8f7b", ink3: "#606252", accent: "#f1d98a", accent2: "#c2ad6c" } },
  { id: "sakura", name: "Sakura", description: "Plum-black planes with a soft cherry-blossom pink.", kind: "dark",
    tokens: { bg0: "#0b0709", bg1: "#110b0e", bg2: "#171014", bg3: "#1f161b", bg4: "#291e24", ink0: "#f6edf1", ink1: "#c9b6bf", ink2: "#94818a", ink3: "#67585f", accent: "#f7b9d2", accent2: "#c693a8" } },
  { id: "ocean", name: "Ocean", description: "Deep-sea blue-green with a clear lagoon accent.", kind: "dark",
    tokens: { bg0: "#040a0c", bg1: "#071013", bg2: "#0b161a", bg3: "#101e23", bg4: "#17282e", ink0: "#e8f3f6", ink1: "#adc3c9", ink2: "#788e95", ink3: "#506368", accent: "#7fd6ea", accent2: "#61a9ba" } },
  { id: "crimson", name: "Crimson", description: "Charcoal with a bold, warm red.", kind: "dark",
    tokens: { bg0: "#0a0707", bg1: "#100b0b", bg2: "#161010", bg3: "#1e1616", bg4: "#281e1e", ink0: "#f6eded", ink1: "#c9b7b7", ink2: "#958282", ink3: "#695959", accent: "#ff9f9f", accent2: "#cc7f7f" } },
  { id: "daylight", name: "Daylight", description: "Light grey planes with slate ink, for bright rooms.", kind: "light",
    tokens: { bg0: "#eef0f3", bg1: "#e6e9ed", bg2: "#fbfbfc", bg3: "#f0f2f5", bg4: "#e2e5ea", ink0: "#14161b", ink1: "#3b404a", ink2: "#5c6370", ink3: "#858b97", accent: "#2d3644", accent2: "#4f5868" } },
  { id: "paper", name: "Paper", description: "Warm off-white with ink-blue accents.", kind: "light",
    tokens: { bg0: "#f2efe9", bg1: "#ebe7df", bg2: "#fcfbf8", bg3: "#f2eee7", bg4: "#e5e0d6", ink0: "#1b1915", ink1: "#433e36", ink2: "#655f55", ink3: "#8d867b", accent: "#27406b", accent2: "#4b5f84" } },
  { id: "arctic", name: "Arctic", description: "Icy blue-white with a deep glacier accent.", kind: "light",
    tokens: { bg0: "#ecf1f5", bg1: "#e3eaf0", bg2: "#fafcfd", bg3: "#eef3f7", bg4: "#dde5ec", ink0: "#0f1720", ink1: "#334250", ink2: "#556574", ink3: "#7e8c99", accent: "#1d5a86", accent2: "#3f6f93" } },
  { id: "latte", name: "Latte", description: "Creamy coffee tones with a roasted brown accent.", kind: "light",
    tokens: { bg0: "#f3ede5", bg1: "#ece4d9", bg2: "#fdfaf6", bg3: "#f4eee6", bg4: "#e7ded2", ink0: "#1f1812", ink1: "#4a3d31", ink2: "#6b5c4e", ink3: "#958677", accent: "#7a4719", accent2: "#8f6440" } },
  { id: "lavender", name: "Lavender", description: "Soft lilac light with a violet accent.", kind: "light",
    tokens: { bg0: "#f0eef8", bg1: "#e8e5f3", bg2: "#fcfbff", bg3: "#f1eff9", bg4: "#e2def0", ink0: "#16131f", ink1: "#3d3750", ink2: "#5f5874", ink3: "#88819c", accent: "#5a3ea8", accent2: "#715ca8" } },
];

/** Page backgrounds that move (shown still with reduce motion). */
export const ANIMATED_SCENES: BackgroundStyle[] = ["aurora", "nebula", "starfield", "waves", "fireflies"];

export function findTheme(id: string, custom: Theme[] = []): Theme {
  return [...OFFICIAL_THEMES, ...custom].find((t) => t.id === id) ?? OFFICIAL_THEMES[0];
}

// ------------------------------------------------------------------ colour

export function hexToRgb(hex: string): [number, number, number] {
  let h = hex.replace("#", "");
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  const n = parseInt(h, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** WCAG relative luminance. */
export function luminance(hex: string): number {
  const [r, g, b] = hexToRgb(hex).map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio between two hex colours (1–21). */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

export function mix(a: string, b: string, t: number): string {
  const [r1, g1, b1] = hexToRgb(a);
  const [r2, g2, b2] = hexToRgb(b);
  const c = (x: number, y: number) => Math.round(x + (y - x) * t).toString(16).padStart(2, "0");
  return `#${c(r1, r2)}${c(g1, g2)}${c(b1, b2)}`;
}

/** Minimum contrast against the panel colour (WCAG: 7 AAA body text, 4.5 AA, 3 for large text/UI). */
export const MIN_CONTRAST = { ink0: 7, ink1: 4.5, ink2: 3, accent: 3 } as const;

/** Why a theme would be hard to read, or null when it's fine. */
export function readabilityProblem(t: ThemeTokens): string | null {
  const checks: [keyof typeof MIN_CONTRAST, string][] = [
    ["ink0", "Main text"],
    ["ink1", "Secondary text"],
    ["ink2", "Muted text"],
    ["accent", "The accent colour"],
  ];
  for (const [k, label] of checks) {
    const c = contrast(t[k], t.bg2);
    if (c < MIN_CONTRAST[k]) return `${label} is too close to the panel colour (contrast ${c.toFixed(1)}:1, needs ${MIN_CONTRAST[k]}:1).`;
  }
  return null;
}

/** Whether a custom accent stays readable on this theme. */
export function accentProblem(accent: string, t: ThemeTokens): string | null {
  const c = contrast(accent, t.bg2);
  return c < MIN_CONTRAST.accent ? `That accent is hard to see on this theme (contrast ${c.toFixed(1)}:1, needs ${MIN_CONTRAST.accent}:1).` : null;
}

// ------------------------------------------------------------------- apply

/** Set every design token for the current preferences. */
export function applyAppearance(prefs: Prefs, el: HTMLElement = document.documentElement) {
  const theme = findTheme(prefs.appearance.theme, prefs.appearance.customThemes);
  const t = theme.tokens;
  const a = prefs.appearance;
  const light = theme.kind === "light";
  // The active workspace can bring its own accent.
  const ws = prefs.workspaces.list.find((w) => w.id === prefs.workspaces.active);
  const wanted = ws?.accent ?? a.accent;
  const accent = wanted && !accentProblem(wanted, t) ? wanted : t.accent;
  const accent2 = accent === t.accent ? t.accent2 : mix(accent, t.bg2, 0.3);
  const [ar, ag, ab] = hexToRgb(accent);
  const hc = a.highContrast;
  const over = light ? "0, 0, 0" : "255, 255, 255";
  const set = (k: string, v: string) => el.style.setProperty(k, v);
  const strong = light ? "#000000" : "#ffffff";

  set("--bg-0", t.bg0);
  set("--bg-1", t.bg1);
  set("--bg-2", t.bg2);
  set("--bg-3", t.bg3);
  set("--bg-4", t.bg4);
  set("--ink-0", hc ? strong : t.ink0);
  set("--ink-1", hc ? mix(t.ink1, strong, 0.5) : t.ink1);
  set("--ink-2", hc ? mix(t.ink2, strong, 0.45) : t.ink2);
  set("--ink-3", hc ? mix(t.ink3, strong, 0.4) : t.ink3);
  set("--overlay-rgb", over);
  set("--line", !a.borders && !hc ? "transparent" : `rgba(${over}, ${hc ? 0.22 : light ? 0.09 : 0.055})`);
  set("--line-2", `rgba(${over}, ${hc ? 0.32 : light ? 0.14 : 0.1})`);
  set("--line-3", `rgba(${over}, ${hc ? 0.45 : light ? 0.22 : 0.16})`);
  set("--accent-rgb", `${ar}, ${ag}, ${ab}`);
  set("--moon", accent);
  // Text on accent-coloured fills (primary buttons, ticked boxes): whichever reads better.
  set("--on-accent", contrast(accent, "#0b0c0f") >= contrast(accent, "#ffffff") ? "#0b0c0f" : "#ffffff");
  set("--moon-2", accent2);
  set("--moon-glow", `rgba(${ar}, ${ag}, ${ab}, ${light ? 0.1 : 0.07})`);
  set("--moon-wash", `rgba(${ar}, ${ag}, ${ab}, ${light ? 0.06 : 0.04})`);
  // Status text tuned per brightness (the fixed status fills stay the same).
  set("--ink-good", light ? "#18751b" : "#7fd07f");
  set("--ink-warning", light ? "#8a5a00" : "#f6cc6e");
  set("--ink-serious", light ? "#a3461f" : "#f2a585");
  set("--ink-critical", light ? "#b42323" : "#ee8a8a");
  set("--grid", mix(t.bg2, t.ink0, light ? 0.1 : 0.08));
  set("--axis", mix(t.bg2, t.ink0, light ? 0.22 : 0.16));
  set("--radius-s", `${Math.round(7 * a.radius)}px`);
  set("--radius", `${Math.round(12 * a.radius)}px`);
  set("--radius-l", `${Math.round(18 * a.radius)}px`);
  set("--radius-xl", `${Math.round(24 * a.radius)}px`);
  // See-through window and chrome colours, used only while Windows shows Mica/Acrylic behind the app.
  const [w0r, w0g, w0b] = hexToRgb(t.bg0);
  const [w1r, w1g, w1b] = hexToRgb(t.bg1);
  const glass = a.backdrop === "acrylic" ? [0.62, 0.5] : [0.5, 0.32];
  set("--window-bg-glass", `rgba(${w0r}, ${w0g}, ${w0b}, ${glass[0]})`);
  set("--chrome-bg-glass", `rgba(${w1r}, ${w1g}, ${w1b}, ${glass[1]})`);
  // Cards sit on the material as lighter layers (as in Windows 11 Settings); firmer on Acrylic, which shows more through.
  const [c2r, c2g, c2b] = hexToRgb(t.bg2);
  set(
    "--panel-bg-glass",
    a.backdrop === "acrylic" ? `rgba(${c2r}, ${c2g}, ${c2b}, ${light ? 0.86 : 0.8})` : light ? "rgba(255, 255, 255, 0.7)" : "rgba(255, 255, 255, 0.045)",
  );
  const [pr, pg, pb] = hexToRgb(t.bg2);
  set("--panel-bg", a.transparency > 0 ? `rgba(${pr}, ${pg}, ${pb}, ${1 - a.transparency})` : t.bg2);
  set("--panel-blur", a.blur > 0 && a.transparency > 0 ? `blur(${a.blur}px)` : "none");
  set("--panel-shadow", a.shadows ? (light ? "0 1px 2px rgba(0,0,0,0.06), 0 6px 18px rgba(0,0,0,0.06)" : "0 8px 24px rgba(0,0,0,0.25)") : "none");
  set("--fs", String(prefs.accessibility.fontScale));
  el.style.colorScheme = theme.kind;
  el.dataset.theme = theme.kind;
  (el.style as CSSStyleDeclaration & { zoom: string }).zoom = String(prefs.accessibility.uiScale);
}
