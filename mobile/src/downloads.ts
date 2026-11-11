// Downloaded songs (saved on the phone by the Swift side, see Downloads.swift).
import { useSyncExternalStore } from "react";
import { api, type Track } from "./api";
import { downloads, type DownloadState } from "./native";

let state: DownloadState = { tracks: [], pending: [], active: null, progress: 0, bytes: 0, failed: [] };
let done = new Set<string>();
let started = false;
const subs = new Set<() => void>();

function apply(s: DownloadState) {
  state = s;
  done = new Set(s.tracks.map((t) => t.videoId));
  subs.forEach((f) => f());
}

function start() {
  if (started) return;
  started = true;
  downloads.state().then(apply).catch(() => {});
  downloads.onChange(apply).catch(() => {});
}

export function useDownloads(): DownloadState {
  start();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => state,
  );
}

export const offline = {
  has: (id: string) => done.has(id),
  /** Downloading now or waiting in line. */
  busy: (id: string) => state.active === id || state.pending.includes(id),
  /** How many of these songs are saved / still on their way. */
  count(tracks: Track[]) {
    let saved = 0;
    let waiting = 0;
    for (const t of tracks) {
      if (done.has(t.videoId)) saved++;
      else if (state.active === t.videoId || state.pending.includes(t.videoId)) waiting++;
    }
    return { saved, waiting };
  },
  async add(tracks: Track[]) {
    start();
    const fresh = tracks.filter((t) => !done.has(t.videoId));
    if (!fresh.length) return;
    await downloads.add(fresh);
    // fetch the lyrics too, so they show offline (saved on the Rust side)
    void (async () => {
      for (const t of fresh.slice(0, 300)) {
        await api.lyrics(t).catch(() => null);
      }
    })();
  },
  remove: (ids: string[]) => downloads.remove(ids),
  removeAll: () => downloads.removeAll(),
};

export function fmtBytes(n: number): string {
  if (n < 1e6) return `${Math.round(n / 1e3)} KB`;
  if (n < 1e9) return `${(n / 1e6).toFixed(n < 1e8 ? 1 : 0)} MB`;
  return `${(n / 1e9).toFixed(2)} GB`;
}
