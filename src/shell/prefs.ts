// The central preference document. Every UI module reads its preferences from
// here instead of keeping its own; Experience Studio edits them. Stored by the
// backend as one JSON document (`ui.prefs`).
//
// `sanitize` is the safety net: whatever is stored (an old version, a hand
// edit, a bug), each field is checked on its own and falls back to its
// default, so one bad value can never make the interface unusable.

import type { PageId } from "../lib/app";
import { OFFICIAL_THEMES, readabilityProblem, type Theme, type ThemeTokens } from "./theme";

export type BackgroundStyle = "plain" | "glow" | "horizon" | "stars";
export const BACKGROUNDS: BackgroundStyle[] = ["plain", "glow", "horizon", "stars"];
const TOKEN_KEYS: (keyof ThemeTokens)[] = ["bg0", "bg1", "bg2", "bg3", "bg4", "ink0", "ink1", "ink2", "ink3", "accent", "accent2"];

/** A stored custom theme, or null if it's malformed or unreadable. */
export function sanitizeTheme(raw: unknown): Theme | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const tok = (typeof r.tokens === "object" && r.tokens !== null ? r.tokens : {}) as Record<string, unknown>;
  if (typeof r.id !== "string" || !/^custom-[a-z0-9-]{1,40}$/.test(r.id)) return null;
  if (OFFICIAL_THEMES.some((t) => t.id === r.id)) return null;
  if (!TOKEN_KEYS.every((k) => isHexColour(tok[k]))) return null;
  const tokens = Object.fromEntries(TOKEN_KEYS.map((k) => [k, (tok[k] as string).toLowerCase()])) as unknown as ThemeTokens;
  if (readabilityProblem(tokens)) return null;
  const name = typeof r.name === "string" && r.name.trim() ? r.name.trim().slice(0, 40) : "My theme";
  return { id: r.id, name, description: typeof r.description === "string" ? r.description.slice(0, 120) : "", kind: r.kind === "light" ? "light" : "dark", tokens };
}

export const PREFS_VERSION = 1;

export type WidgetType = "vitals" | "health" | "storage" | "network" | "quick" | "music" | "apps" | "ask" | "gaming";
export type WidgetSize = "s" | "m" | "l";
export interface WidgetSlot {
  /** Unique per slot, so the same widget type can appear twice. */
  id: string;
  type: WidgetType;
  size: WidgetSize;
}

export type PlayerMode = "full" | "standard" | "mini" | "minimal";
export type SidebarSide = "left" | "right";
export type IntroMode = "full" | "short" | "off";

export interface Prefs {
  version: number;
  appearance: {
    /** ID of an official or custom theme. */
    theme: string;
    /** Overrides the theme's accent; null = the theme's own. */
    accent: string | null;
    /** Corner radius scale, 0 (square) – 2 (very round); 1 = default. */
    radius: number;
    /** Panel transparency 0–0.6 (0 = opaque). */
    transparency: number;
    /** Background blur in px behind translucent panels, 0–24. */
    blur: number;
    /** Panel shadows. */
    shadows: boolean;
    /** Panel borders. */
    borders: boolean;
    highContrast: boolean;
    /** Treatment behind the pages. */
    background: BackgroundStyle;
    /** The user's own themes (Experience Studio). */
    customThemes: Theme[];
    /** Startup animation. */
    intro: IntroMode;
  };
  navigation: {
    /** Section order (section IDs). Unknown IDs are dropped, missing ones appended. */
    order: string[];
    /** Hidden section IDs (System and Settings can't be hidden). */
    hidden: string[];
    /** Pinned page IDs, shown at the top of the sidebar. */
    pinned: PageId[];
    /** Sections expanded in the sidebar. */
    expanded: string[];
    startPage: PageId;
    sidebar: SidebarSide;
    compact: boolean;
  };
  home: {
    widgets: WidgetSlot[];
  };
  music: {
    playerMode: PlayerMode;
    volume: number;
    visualiser: boolean;
    showLyrics: boolean;
  };
  workspaces: {
    /** ID of the active workspace; null = none (plain Nocturn). */
    active: string | null;
    list: Workspace[];
    /** Workspace to switch to when Nocturn opens; null = keep the last one. */
    startWith: string | null;
    /** Ask before opening a workspace's apps when switching. */
    confirmLaunch: boolean;
  };
  accessibility: {
    reduceMotion: boolean;
    /** Text size multiplier 0.85–1.4. */
    fontScale: number;
    /** Whole-interface zoom 0.8–1.5. */
    uiScale: number;
  };
}

export const DEFAULT_WIDGETS: WidgetSlot[] = [
  { id: "w-vitals", type: "vitals", size: "l" },
  { id: "w-quick", type: "quick", size: "m" },
  { id: "w-health", type: "health", size: "m" },
  { id: "w-music", type: "music", size: "m" },
  { id: "w-apps", type: "apps", size: "m" },
  { id: "w-storage", type: "storage", size: "s" },
  { id: "w-network", type: "network", size: "s" },
  { id: "w-ask", type: "ask", size: "m" },
];

export const DEFAULT_PREFS: Prefs = {
  version: PREFS_VERSION,
  appearance: { theme: "nocturn", accent: null, radius: 1, transparency: 0, blur: 0, shadows: true, borders: true, highContrast: false, background: "plain", customThemes: [], intro: "full" },
  navigation: {
    order: ["home", "library", "music", "workspaces", "ai", "gaming", "system", "studio", "settings"],
    hidden: [],
    pinned: [],
    expanded: ["system"],
    startPage: "home",
    sidebar: "left",
    compact: false,
  },
  home: { widgets: DEFAULT_WIDGETS },
  music: { playerMode: "standard", volume: 0.8, visualiser: true, showLyrics: true },
  workspaces: { active: null, list: [], startWith: null, confirmLaunch: true },
  accessibility: { reduceMotion: false, fontScale: 1, uiScale: 1 },
};

/** Sections that can never be hidden, so the user can always get back. */
export const LOCKED_SECTIONS = ["system", "settings"];
export const WIDGET_TYPES: WidgetType[] = ["vitals", "health", "storage", "network", "quick", "music", "apps", "ask", "gaming"];

// ------------------------------------------------------------------ checks

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const num = (v: unknown, min: number, max: number, d: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : d);
const bool = (v: unknown, d: boolean) => (typeof v === "boolean" ? v : d);
const str = (v: unknown, d: string, max = 80) => (typeof v === "string" && v.length > 0 && v.length <= max ? v : d);
const oneOf = <T extends string>(v: unknown, allowed: readonly T[], d: T): T => (allowed.includes(v as T) ? (v as T) : d);
const strList = (v: unknown, allowed?: readonly string[], max = 50) =>
  Array.isArray(v) ? [...new Set(v.filter((x): x is string => typeof x === "string" && (!allowed || allowed.includes(x))))].slice(0, max) : null;

/** A widget list, or null if it isn't a list at all. Unknown or duplicate widgets are dropped. */
export function sanitizeWidgets(raw: unknown): WidgetSlot[] | null {
  if (!Array.isArray(raw)) return null;
  const seen = new Set<string>();
  return raw
    .filter(isObj)
    .map((s) => ({ id: str(s.id, "", 40), type: oneOf(s.type, WIDGET_TYPES, "vitals"), size: oneOf<WidgetSize>(s.size, ["s", "m", "l"], "m"), ok: WIDGET_TYPES.includes(s.type as WidgetType) }))
    .filter((s) => s.ok && s.id && !seen.has(s.id) && seen.add(s.id))
    .map(({ id, type, size }) => ({ id, type, size }))
    .slice(0, 24);
}

export interface Workspace {
  id: string;
  name: string;
  /** One or two characters (emoji or letters). */
  glyph: string;
  /** Accent while this workspace is active; null = the theme's. */
  accent: string | null;
  /** Page background while active; null = the global setting. */
  background: BackgroundStyle | null;
  /** Library item IDs in this workspace. */
  apps: string[];
  /** Library item IDs opened when switching to this workspace. */
  launch: string[];
  /** Its own Home dashboard; null = use the main one. */
  widgets: WidgetSlot[] | null;
  /** Nocturn Music playlist to offer when switching; null = none. */
  playlist: string | null;
  description: string;
}

/** A stored workspace, or null if it's malformed. */
export function sanitizeWorkspace(raw: unknown): Workspace | null {
  if (!isObj(raw)) return null;
  if (typeof raw.id !== "string" || !/^ws-[a-z0-9-]{1,40}$/.test(raw.id)) return null;
  const ids = (v: unknown) => (strList(v, undefined, 60) ?? []).filter((x) => x.length <= 64);
  const apps = ids(raw.apps);
  return {
    id: raw.id,
    name: str(typeof raw.name === "string" ? raw.name.trim() : "", "Workspace", 40),
    glyph: typeof raw.glyph === "string" ? [...raw.glyph.trim()].slice(0, 2).join("") : "",
    accent: isHexColour(raw.accent) ? raw.accent : null,
    background: BACKGROUNDS.includes(raw.background as BackgroundStyle) ? (raw.background as BackgroundStyle) : null,
    apps,
    launch: ids(raw.launch).filter((x) => apps.includes(x)),
    widgets: sanitizeWidgets(raw.widgets),
    playlist: typeof raw.playlist === "string" && raw.playlist.length <= 64 ? raw.playlist : null,
    description: typeof raw.description === "string" ? raw.description.slice(0, 120) : "",
  };
}

/** #rgb or #rrggbb only. */
export const isHexColour = (v: unknown): v is string => typeof v === "string" && /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(v);

/**
 * Turn anything that was stored into a valid preference document.
 * `pages` / `sections` are the IDs that exist in this build.
 */
export function sanitize(raw: unknown, pages: readonly string[], sections: readonly string[]): Prefs {
  const d = DEFAULT_PREFS;
  const r = isObj(raw) ? raw : {};
  const a = isObj(r.appearance) ? r.appearance : {};
  const n = isObj(r.navigation) ? r.navigation : {};
  const h = isObj(r.home) ? r.home : {};
  const m = isObj(r.music) ? r.music : {};
  const w = isObj(r.workspaces) ? r.workspaces : {};
  const x = isObj(r.accessibility) ? r.accessibility : {};

  // Order: keep the stored order of known sections, append any new ones.
  const stored = strList(n.order, sections) ?? [];
  const order = [...stored, ...d.navigation.order.filter((s) => sections.includes(s) && !stored.includes(s)), ...sections.filter((s) => !stored.includes(s) && !d.navigation.order.includes(s))];
  const hidden = (strList(n.hidden, sections) ?? []).filter((s) => !LOCKED_SECTIONS.includes(s));

  const customThemes: Theme[] = [];
  if (Array.isArray(a.customThemes)) {
    for (const t of a.customThemes.slice(0, 30)) {
      const ok = sanitizeTheme(t);
      if (ok && !customThemes.some((x) => x.id === ok.id)) customThemes.push(ok);
    }
  }
  const workspaces: Workspace[] = [];
  if (Array.isArray(w.list)) {
    for (const raw of w.list.slice(0, 20)) {
      const ws = sanitizeWorkspace(raw);
      if (ws && !workspaces.some((x) => x.id === ws.id)) workspaces.push(ws);
    }
  }
  const widgets = sanitizeWidgets(h.widgets) ?? d.home.widgets;

  return {
    version: PREFS_VERSION,
    appearance: {
      theme: str(a.theme, d.appearance.theme, 40),
      accent: isHexColour(a.accent) ? a.accent : null,
      radius: num(a.radius, 0, 2, d.appearance.radius),
      transparency: num(a.transparency, 0, 0.6, d.appearance.transparency),
      blur: num(a.blur, 0, 24, d.appearance.blur),
      shadows: bool(a.shadows, d.appearance.shadows),
      borders: bool(a.borders, d.appearance.borders),
      highContrast: bool(a.highContrast, d.appearance.highContrast),
      background: oneOf<BackgroundStyle>(a.background, BACKGROUNDS, "plain"),
      customThemes,
      intro: oneOf<IntroMode>(a.intro, ["full", "short", "off"], d.appearance.intro),
    },
    navigation: {
      order,
      hidden,
      pinned: (strList(n.pinned, pages, 12) ?? []) as PageId[],
      expanded: strList(n.expanded, sections) ?? d.navigation.expanded,
      startPage: oneOf(n.startPage, pages as readonly PageId[], d.navigation.startPage),
      sidebar: oneOf<SidebarSide>(n.sidebar, ["left", "right"], "left"),
      compact: bool(n.compact, false),
    },
    home: { widgets },
    music: {
      playerMode: oneOf<PlayerMode>(m.playerMode, ["full", "standard", "mini", "minimal"], d.music.playerMode),
      volume: num(m.volume, 0, 1, d.music.volume),
      visualiser: bool(m.visualiser, d.music.visualiser),
      showLyrics: bool(m.showLyrics, d.music.showLyrics),
    },
    workspaces: {
      active: typeof w.active === "string" && workspaces.some((x) => x.id === w.active) ? w.active : null,
      list: workspaces,
      startWith: typeof w.startWith === "string" && workspaces.some((x) => x.id === w.startWith) ? w.startWith : null,
      confirmLaunch: bool(w.confirmLaunch, true),
    },
    accessibility: {
      reduceMotion: bool(x.reduceMotion, d.accessibility.reduceMotion),
      fontScale: num(x.fontScale, 0.85, 1.4, 1),
      uiScale: num(x.uiScale, 0.8, 1.5, 1),
    },
  };
}
