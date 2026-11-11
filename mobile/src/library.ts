// Your library, kept on the phone (library.json in the app's data folder).
import { useSyncExternalStore } from "react";
import { api, type Card, type Track } from "./api";

export type Library = {
  version: 1;
  liked: Track[]; // newest first
  albums: Card[];
  playlists: Card[];
  artists: Card[];
  recent: Track[]; // recently played, newest first
  searches: string[];
};

const blank: Library = { version: 1, liked: [], albums: [], playlists: [], artists: [], recent: [], searches: [] };

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
  clearSearches() {
    set({ ...lib, searches: [] });
  },
};
