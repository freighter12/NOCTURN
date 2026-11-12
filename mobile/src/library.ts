// Your library, kept on the phone (library.json in the app's data folder).
import { useSyncExternalStore } from "react";
import { api, type Card, type Track } from "./api";
import type { HomePrefs } from "./greeting";
import { setHapticsOn } from "./native";

export type MyPlaylist = {
  id: string;
  title: string;
  source?: string;
  url?: string;
  artwork?: string | null;
  tracks: Track[];
  missing?: { title: string; artist: string }[];
  created: number;
};

export type Library = {
  version: 1;
  mine: MyPlaylist[]; // your playlists (imported), newest first
  liked: Track[]; // newest first
  albums: Card[];
  playlists: Card[];
  artists: Card[];
  recent: Track[]; // recently played, newest first
  searches: string[];
  /** Settings › Home Screen (see greeting.ts) */
  home?: Partial<HomePrefs>;
  /** small app preferences kept with the library */
  prefs?: { haptics?: boolean; motion?: Motion };
};

/** page animations: Settings › Appearance */
export type Motion = "full" | "subtle" | "off";

function applyMotion(m: Motion | undefined) {
  document.documentElement.dataset.motion = m ?? "full";
}

const blank: Library = { version: 1, mine: [], liked: [], albums: [], playlists: [], artists: [], recent: [], searches: [] };

let lib: Library = blank;
let loaded = false;
const subs = new Set<() => void>();
let saveTimer: ReturnType<typeof setTimeout> | undefined;

function emit() {
  subs.forEach((s) => s());
}

function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    api.librarySave(JSON.stringify(lib)).catch((e) => console.warn("library save failed", e));
  }, 400);
}

function set(next: Library) {
  lib = next;
  emit();
  save();
}

export async function loadLibrary() {
  if (loaded) return;
  loaded = true;
  try {
    const parsed = JSON.parse(await api.libraryLoad());
    lib = { ...blank, ...parsed, version: 1 };
    setHapticsOn(lib.prefs?.haptics ?? true);
    applyMotion(lib.prefs?.motion);
  } catch {
    lib = blank;
  }
  emit();
}

export function useLibrary(): Library {
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => lib,
  );
}

export const library = {
  get: () => lib,
  isLiked: (id: string) => lib.liked.some((t) => t.videoId === id),
  toggleLike(t: Track) {
    const has = lib.liked.some((x) => x.videoId === t.videoId);
    set({ ...lib, liked: has ? lib.liked.filter((x) => x.videoId !== t.videoId) : [t, ...lib.liked] });
    return !has;
  },
  isSaved: (c: { id: string }) => [...lib.albums, ...lib.playlists, ...lib.artists].some((x) => x.id === c.id),
  toggleSaved(c: Card) {
    const key = c.kind === "album" ? "albums" : c.kind === "artist" ? "artists" : "playlists";
    const list = lib[key];
    const has = list.some((x) => x.id === c.id);
    set({ ...lib, [key]: has ? list.filter((x) => x.id !== c.id) : [c, ...list] });
    return !has;
  },
  played(t: Track) {
    if (lib.recent[0]?.videoId === t.videoId) return;
    set({ ...lib, recent: [t, ...lib.recent.filter((x) => x.videoId !== t.videoId)].slice(0, 60) });
  },
  searched(q: string) {
    const s = q.trim();
    if (!s) return;
    set({ ...lib, searches: [s, ...lib.searches.filter((x) => x.toLowerCase() !== s.toLowerCase())].slice(0, 12) });
  },
  playlist: (id: string) => lib.mine.find((p) => p.id === id),
  addPlaylist(p: Omit<MyPlaylist, "id" | "created">): MyPlaylist {
    const full: MyPlaylist = { ...p, id: `mine:${Date.now().toString(36)}`, created: Date.now() };
    set({ ...lib, mine: [full, ...lib.mine] });
    return full;
  },
  removePlaylist(id: string) {
    set({ ...lib, mine: lib.mine.filter((p) => p.id !== id) });
  },
  removeFromPlaylist(id: string, videoId: string) {
    set({ ...lib, mine: lib.mine.map((p) => (p.id === id ? { ...p, tracks: p.tracks.filter((t) => t.videoId !== videoId) } : p)) });
  },
  addToPlaylist(id: string, t: Track) {
    set({ ...lib, mine: lib.mine.map((p) => (p.id === id && !p.tracks.some((x) => x.videoId === t.videoId) ? { ...p, tracks: [...p.tracks, t] } : p)) });
  },
  clearSearches() {
    set({ ...lib, searches: [] });
  },
  clearRecent() {
    set({ ...lib, recent: [] });
  },
  setHome(patch: Partial<HomePrefs> | null) {
    set({ ...lib, home: patch === null ? {} : { ...lib.home, ...patch } });
  },
  setMotion(m: Motion) {
    applyMotion(m);
    set({ ...lib, prefs: { ...lib.prefs, motion: m } });
  },
  setHaptics(on: boolean) {
    setHapticsOn(on);
    set({ ...lib, prefs: { ...lib.prefs, haptics: on } });
  },
};
