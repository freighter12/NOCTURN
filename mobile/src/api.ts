// Rust commands (src-tauri/src/lib.rs) and the shapes they return.
import { invoke } from "@tauri-apps/api/core";
import { demoShelves, trailerOn } from "./trailer";

export type Track = {
  videoId: string;
  title: string;
  artist: string;
  artistId?: string | null;
  album?: string | null;
  albumId?: string | null;
  durationMs?: number | null;
  artwork?: string | null;
  explicit?: boolean;
  video?: boolean;
};

export type CardKind = "album" | "playlist" | "artist" | "song" | "mine";

export type Card = {
  kind: CardKind;
  id: string;
  title: string;
  subtitle: string;
  artwork?: string | null;
};

export type Shelf = { title: string; tracks: Track[]; cards: Card[] };

export type SearchResults = {
  top: Card | null;
  songs: Track[];
  videos: Track[];
  albums: Card[];
  artists: Card[];
  playlists: Card[];
};

export type Collection = {
  id: string;
  kind: "album" | "playlist";
  title: string;
  subtitle: string;
  artist: string;
  artwork?: string | null;
  description: string;
  tracks: Track[];
};

export type ArtistPage = {
  id: string;
  name: string;
  artwork?: string | null;
  topSongs: Track[];
  shelves: Shelf[];
};

export type Lyrics = { text: string; synced: [number, string][]; words: [number, string][][]; approx: boolean; instrumental: boolean };

export type ImportResult = {
  title: string;
  source: string;
  url: string;
  artwork?: string | null;
  tracks: Track[];
  missing: { title: string; artist: string }[];
  /** Spotify only showed the first 100 songs of a long playlist. */
  truncated: boolean;
};

export type AIPlan = { upcoming: Track[]; because: string; discoveries: number };

export type Stream = { url: string; availableAtMs?: number | null };

// Small in-memory cache so going back to a page is instant.
const cache = new Map<string, { at: number; value: unknown }>();
async function cached<T>(key: string, ttlMs: number, load: () => Promise<T>): Promise<T> {
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < ttlMs) return hit.value as T;
  const value = await load();
  cache.set(key, { at: Date.now(), value });
  if (cache.size > 80) cache.delete(cache.keys().next().value as string);
  return value;
}

export const api = {
  search: (query: string) => cached(`s:${query}`, 10 * 60_000, () => invoke<SearchResults>("yt_search", { query })),
  suggest: (input: string) => invoke<string[]>("yt_suggest", { input }),
  home: () => (trailerOn ? Promise.resolve(demoShelves) : cached("home", 20 * 60_000, () => invoke<Shelf[]>("yt_home"))),
  collection: (id: string) => cached(`c:${id}`, 30 * 60_000, () => invoke<Collection>("yt_collection", { id })),
  artist: (id: string) => cached(`a:${id}`, 30 * 60_000, () => invoke<ArtistPage>("yt_artist", { id })),
  radio: (videoId: string) => invoke<Track[]>("yt_radio", { videoId }),
  lyrics: (t: Track) =>
    cached(`l:${t.videoId}`, 24 * 3600_000, () =>
      invoke<Lyrics | null>("lyrics_get", { artist: t.artist, title: t.title, album: t.album ?? null, durationMs: t.durationMs ?? null }),
    ),
  aiShuffle: (input: { current: Track | null; upcoming: Track[]; recent: Track[]; liked: Track[]; skipped: string[]; count?: number }) =>
    invoke<AIPlan>("ai_shuffle", { input }),
  importPlaylist: (link: string) => invoke<ImportResult>("import_playlist", { link }),
  stream: (videoId: string) => invoke<Stream>("yt_stream", { videoId }),
  libraryLoad: () => invoke<string>("library_load"),
  librarySave: (json: string) => invoke<void>("library_save", { json }),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

export function fmtTime(ms?: number | null): string {
  if (!ms || ms < 0) return "";
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}
