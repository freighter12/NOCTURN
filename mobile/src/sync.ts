// Sync with NOCTURN on the PC (Settings › PC Sync). Pair once by scanning the
// code the PC shows; then playlists and liked songs go both ways over Wi-Fi.
import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Track } from "./api";
import { library } from "./library";
import { isNative } from "./native";

export type Pairing = { host: string; port: number; key: string; name: string };
type SyncTrack = { title: string; artist: string; album: string; durationMs?: number | null; videoId?: string | null };
type Pulled = { device: string; playlists: { id: string; name: string; tracks: Track[]; missing: { title: string; artist: string }[] }[]; liked: Track[] };
type Pushed = { device: string; playlists: number; songs: number };

export type SyncState = {
  busy: boolean;
  progress: { done: number; total: number } | null;
  error: string | null;
};

let state: SyncState = { busy: false, progress: null, error: null };
const subs = new Set<() => void>();
const set = (p: Partial<SyncState>) => {
  state = { ...state, ...p };
  subs.forEach((f) => f());
};

export function useSync(): SyncState {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => state,
  );
}

const toSync = (t: Track): SyncTrack => ({ title: t.title, artist: t.artist, album: t.album ?? "", durationMs: t.durationMs ?? null, videoId: t.videoId });

export const sync = {
  /** Open the camera, read the PC's code, check the PC answers, remember it. */
  async pair(): Promise<Pairing> {
    if (!isNative) throw new Error("Pairing works on the iPhone");
    const { code } = await invoke<{ code: string }>("plugin:nocturn|scan_qr");
    const pairing = await invoke<Pairing>("sync_parse_code", { code });
    const name = await invoke<string>("sync_hello", { pairing });
    const full = { ...pairing, name: name || pairing.name };
    library.setSync({ pairing: full, auto: true });
    return full;
  },

  unpair() {
    library.setSync(null);
  },

  /** Bring the PC's playlists and liked songs over, then send the phone's. */
  async run(opts: { quiet?: boolean } = {}): Promise<{ pulled: number; liked: number; pushed: Pushed } | null> {
    const s = library.get().sync;
    if (!s || state.busy) return null;
    set({ busy: true, progress: null, error: null });
    const off = await listen<{ done: number; total: number }>("sync-progress", (e) => set({ progress: e.payload }));
    try {
      const pulled = await invoke<Pulled>("sync_pull", { pairing: s.pairing });
      const likedBefore = library.get().liked.length;
      library.mergeFromPC(pulled.playlists, pulled.liked);
      const lib = library.get();
      const pushed = await invoke<Pushed>("sync_push", {
        pairing: s.pairing,
        library: {
          device: "iPhone",
          playlists: lib.mine.filter((p) => !p.id.startsWith("pc:")).map((p) => ({ id: p.id, name: p.title, tracks: p.tracks.map(toSync) })),
          liked: lib.liked.map(toSync),
        },
      });
      const result = { pulled: pulled.playlists.length, liked: library.get().liked.length - likedBefore, pushed };
      library.setSync({ ...s, last: { at: Date.now(), pulled: result.pulled, pushed: pushed.playlists, liked: result.liked } });
      set({ busy: false, progress: null });
      return result;
    } catch (e) {
      set({ busy: false, progress: null, error: opts.quiet ? null : String(e) });
      if (!opts.quiet) throw e;
      return null;
    } finally {
      off();
    }
  },

  /** Quietly, a few seconds after the app opens, when paired and allowed. */
  startup() {
    if (!isNative) return;
    setTimeout(() => {
      const s = library.get().sync;
      if (s?.auto) sync.run({ quiet: true }).catch(() => {});
    }, 6000);
  },
};
