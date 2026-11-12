// Trailer mode (simulator only: SIMCTL_CHILD_NOCTURN_DEMO=trailer). The
// native side runs a timeline (Trailer.swift) and tells the web side what to
// show. Everything here is made up: invented songs and artists, cover art
// drawn on the fly, no account, no network, nothing personal.
import type { Card, Shelf, Track } from "./api";

import { useSyncExternalStore } from "react";

export let trailerOn = false;
const subs = new Set<() => void>();

/** Re-renders when trailer mode switches on (Home swaps to the demo shelves). */
export function useTrailer(): boolean {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => trailerOn,
  );
}

/** Gradient cover art, drawn as an SVG (no real album covers in a trailer). */
function cover(i: number, shape: "moon" | "waves" | "rings" | "grid" | "sun" = "moon"): string {
  const palettes = [
    ["#5b3cc4", "#ff7a59"], ["#0f6b8f", "#7cf2d1"], ["#9b2f6b", "#ffb36b"], ["#1d2a6b", "#8fb4ff"],
    ["#2b7a3d", "#e6ff8a"], ["#6b1d2a", "#ff8fa3"], ["#3a1d6b", "#d18fff"], ["#7a4a12", "#ffd36b"],
  ];
  const [a, b] = palettes[i % palettes.length];
  const art = {
    moon: `<circle cx="300" cy="300" r="150" fill="#fff" opacity=".92"/><circle cx="360" cy="250" r="135" fill="url(#g)"/>`,
    waves: [0, 1, 2, 3].map((k) => `<path d="M0 ${330 + k * 55} Q150 ${280 + k * 55} 300 ${330 + k * 55} T600 ${330 + k * 55}" stroke="#fff" stroke-opacity="${0.75 - k * 0.15}" stroke-width="14" fill="none"/>`).join(""),
    rings: [60, 110, 160, 210].map((r, k) => `<circle cx="300" cy="300" r="${r}" stroke="#fff" stroke-opacity="${0.85 - k * 0.18}" stroke-width="12" fill="none"/>`).join(""),
    grid: Array.from({ length: 7 }, (_, k) => `<line x1="0" y1="${360 + k * 36}" x2="600" y2="${360 + k * 36}" stroke="#fff" stroke-opacity=".5" stroke-width="4"/><line x1="${300 + (k - 3) * 40}" y1="360" x2="${300 + (k - 3) * 160}" y2="600" stroke="#fff" stroke-opacity=".5" stroke-width="4"/>`).join("") + `<circle cx="300" cy="250" r="110" fill="#fff" opacity=".9"/>`,
    sun: `<circle cx="300" cy="330" r="170" fill="#fff" opacity=".9"/><rect x="0" y="380" width="600" height="220" fill="url(#g)" opacity=".85"/>`,
  }[shape];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 600"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="600" height="600" fill="url(#g)"/>${art}</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

const SHAPES = ["moon", "waves", "rings", "grid", "sun"] as const;
const SONGS: [string, string][] = [
  ["Moonlight Drive", "Lunar Echo"], ["Velvet Static", "Neon Harbour"], ["Paper Comets", "Glass Avenue"], ["After Hours", "Midnight Arcade"],
  ["Silver Lines", "Ember Lane"], ["Slow Motion", "Nova Bloom"], ["City Rain", "Lunar Echo"], ["Starlight Club", "Orbit Kids"],
  ["Northern Glow", "Polar Soda"], ["Echo Park", "Neon Harbour"], ["Golden Hour", "Sunday Static"], ["Night Swim", "Glass Avenue"],
];

export const demoTracks: Track[] = SONGS.map(([title, artist], i) => ({
  videoId: `demo${String(i).padStart(7, "0")}`,
  title,
  artist,
  album: title,
  durationMs: 180_000 + i * 7_000,
  artwork: cover(i, SHAPES[i % SHAPES.length]),
}));

const card = (kind: Card["kind"], i: number, title: string, subtitle: string): Card => ({ kind, id: `demo-${kind}-${i}`, title, subtitle, artwork: cover(i + 3, SHAPES[(i + 2) % SHAPES.length]) });

export const demoShelves: Shelf[] = [
  { title: "Quick picks", tracks: demoTracks.slice(0, 8), cards: [] },
  { title: "Your mixes", tracks: [], cards: [card("playlist", 0, "Late Night Mix", "Lunar Echo, Neon Harbour and more"), card("playlist", 1, "Focus Flow", "Glass Avenue, Nova Bloom"), card("playlist", 2, "Gym Energy", "Orbit Kids, Midnight Arcade"), card("playlist", 3, "Chill Sunday", "Sunday Static, Polar Soda")] },
  { title: "New for you", tracks: [], cards: [card("album", 4, "Neon Tides", "Album · Neon Harbour"), card("album", 5, "Paper Skies", "Album · Glass Avenue"), card("album", 6, "Orbit", "EP · Orbit Kids"), card("album", 7, "Lunar", "Album · Lunar Echo")] },
];

export function enableTrailer() {
  trailerOn = true;
  subs.forEach((f) => f());
}
