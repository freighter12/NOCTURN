// Data-driven themes. A theme is a set of design tokens; applying one just
// sets CSS variables on :root, so switching is instant and nothing is
// hard-coded per theme. Custom themes (Experience Studio) use the same shape.

import type { Prefs } from "./prefs";

export interface ThemeTokens {
  bg0: string; bg1: string; bg2: string; bg3: string; bg4: string;
  ink0: string; ink1: string; ink2: string; ink3: string;
  accent: string; accent2: string;
  line: string; line2: string; line3: string;
}

export interface Theme {
  id: string;
  name: string;
  description: string;
  kind: "dark" | "light";
  tokens: ThemeTokens;
}

const lines = (rgb: string) => ({ line: `rgba(${rgb}, 0.055)`, line2: `rgba(${rgb}, 0.1)`, line3: `rgba(${rgb}, 0.16)` });

export const OFFICIAL_THEMES: Theme[] = [
  {
    id: "nocturn",
    name: "Nocturn",
    description: "Deep charcoal with moonlight silver — the original.",
    kind: "dark",
    tokens: { bg0: "#08080a", bg1: "#0d0e11", bg2: "#121317", bg3: "#181a1f", bg4: "#20232a", ink0: "#eceef2", ink1: "#b4b8c2", ink2: "#7d828d", ink3: "#555a64", accent: "#dfe2e8", accent2: "#aeb3be", ...lines("255, 255, 255") },
  },
  {
    id: "eclipse",
    name: "Eclipse",
    description: "True black planes for OLED screens.",
    kind: "dark",
    tokens: { bg0: "#000000", bg1: "#030304", bg2: "#08090b", bg3: "#101114", bg4: "#17191d", ink0: "#f2f3f5", ink1: "#b9bcc4", ink2: "#80848e", ink3: "#575b64", accent: "#e8eaee", accent2: "#a9adb7", ...lines("255, 255, 255") },
  },
  {
    id: "dusk",
    name: "Dusk",
    description: "Night-blue planes with a pale blue moon.",
    kind: "dark",
    tokens: { bg0: "#070910", bg1: "#0b0e17", bg2: "#10141f", bg3: "#161b28", bg4: "#1e2433", ink0: "#e9edf6", ink1: "#b0b8ca", ink2: "#7a8399", ink3: "#525a6d", accent: "#b9cdf5", accent2: "#8d9fc4", ...lines("200, 215, 255") },
  },
  {
    id: "ember",
    name: "Ember",
    description: "Warm charcoal with a soft amber glow.",
    kind: "dark",
    tokens: { bg0: "#0a0807", bg1: "#100d0b", bg2: "#16120f", bg3: "#1d1814", bg4: "#26201b", ink0: "#f3eee9", ink1: "#c3b8ad", ink2: "#8c8177", ink3: "#5f564e", accent: "#f0c89a", accent2: "#c2a07b", ...lines("255, 235, 215") },
  },
  {
    id: "aurora",
    name: "Aurora",
    description: "Deep green-black with a mint accent.",
    kind: "dark",
    tokens: { bg0: "#060908", bg1: "#0a0e0d", bg2: "#0f1513", bg3: "#151c1a", bg4: "#1c2522", ink0: "#e9f2ef", ink1: "#b2c2bd", ink2: "#7b8d88", ink3: "#53625e", accent: "#a6e3cf", accent2: "#80b3a3", ...lines("215, 255, 240") },
  },
];

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

function mix(a: string, b: string, t: number): string {
  const [r1, g1, b1] = hexToRgb(a);
  const [r2, g2, b2] = hexToRgb(b);
  const c = (x: number, y: number) => Math.round(x + (y - x) * t).toString(16).padStart(2, "0");
  return `#${c(r1, r2)}${c(g1, g2)}${c(b1, b2)}`;
}

// ------------------------------------------------------------------- apply

/** Set every design token for the current preferences. */
export function applyAppearance(prefs: Prefs, custom: Theme[] = [], el: HTMLElement = document.documentElement) {
  const t = findTheme(prefs.appearance.theme, custom).tokens;
  const a = prefs.appearance;
  const accent = a.accent ?? t.accent;
  const accent2 = a.accent ? mix(a.accent, t.bg2, 0.3) : t.accent2;
  const [ar, ag, ab] = hexToRgb(accent);
  const hc = a.highContrast;
  const set = (k: string, v: string) => el.style.setProperty(k, v);

  set("--bg-0", t.bg0);
  set("--bg-1", t.bg1);
  set("--bg-2", t.bg2);
  set("--bg-3", t.bg3);
  set("--bg-4", t.bg4);
  set("--ink-0", hc ? "#ffffff" : t.ink0);
  set("--ink-1", hc ? mix(t.ink1, "#ffffff", 0.5) : t.ink1);
  set("--ink-2", hc ? mix(t.ink2, "#ffffff", 0.45) : t.ink2);
  set("--ink-3", hc ? mix(t.ink3, "#ffffff", 0.4) : t.ink3);
  set("--line", hc ? t.line3 : a.borders ? t.line : "transparent");
  set("--line-2", hc ? "rgba(255,255,255,0.28)" : t.line2);
  set("--line-3", hc ? "rgba(255,255,255,0.4)" : t.line3);
  set("--moon", accent);
  set("--moon-2", accent2);
  set("--moon-glow", `rgba(${ar}, ${ag}, ${ab}, 0.07)`);
  set("--moon-wash", `rgba(${ar}, ${ag}, ${ab}, 0.04)`);
  set("--radius-s", `${Math.round(5 * a.radius)}px`);
  set("--radius", `${Math.round(8 * a.radius)}px`);
  set("--radius-l", `${Math.round(12 * a.radius)}px`);
  const [pr, pg, pb] = hexToRgb(t.bg2);
  set("--panel-bg", a.transparency > 0 ? `rgba(${pr}, ${pg}, ${pb}, ${1 - a.transparency})` : t.bg2);
  set("--panel-blur", a.blur > 0 && a.transparency > 0 ? `blur(${a.blur}px)` : "none");
  set("--panel-shadow", a.shadows ? "0 1px 0 rgba(255,255,255,0.02) inset, 0 8px 24px rgba(0,0,0,0.25)" : "none");
  set("--fs", String(prefs.accessibility.fontScale));
  el.style.colorScheme = findTheme(a.theme, custom).kind;
  (el.style as CSSStyleDeclaration & { zoom: string }).zoom = String(prefs.accessibility.uiScale);
}
