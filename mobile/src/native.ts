// The native player and Liquid Glass tab bar (plugins/nocturn, Swift).
// When the app runs on a computer for testing there is no Swift side, so a
// small HTML <audio> stand-in plays instead and the web draws its own tab bar.
import { addPluginListener, invoke } from "@tauri-apps/api/core";
import { api, type Track } from "./api";

export type PlayerState = {
  playing: boolean;
  loading: boolean;
  index: number;
  positionMs: number;
  durationMs: number;
  queueLength: number;
  track: Track | null;
  repeatMode: "off" | "all" | "one";
  autoplay: boolean;
  error: string | null;
};

export type NativeLyricItem = {
  kind: "line" | "gap";
  start: number;
  end: number;
  text: string;
  words: { t: number; w: string }[];
  estimated?: boolean;
  sungEnd?: number;
};

export type TabSpec = { id: string; title: string; symbol: string; search?: boolean };

export const isNative = /iPhone|iPad|iPod/.test(navigator.userAgent);

const P = "plugin:nocturn|";
const call = <T = void>(cmd: string, args?: Record<string, unknown>) => invoke<T>(P + cmd, args);

// Only the fields Swift reads, so the bridge stays small.
const slim = (t: Track) => ({
  videoId: t.videoId,
  title: t.title,
  artist: t.artist,
  album: t.album ?? null,
  artwork: t.artwork ?? null,
  durationMs: t.durationMs ?? null,
});

// ---------------------------------------------------------------- stand-in player (computer only)

const empty: PlayerState = {
  playing: false, loading: false, index: -1, positionMs: 0, durationMs: 0, queueLength: 0,
  track: null, repeatMode: "off", autoplay: true, error: null,
};

class WebPlayer {
  audio = new Audio();
  queue: Track[] = [];
  state: PlayerState = { ...empty };
  listeners = new Set<(s: PlayerState) => void>();
  token = 0;

  constructor() {
    const a = this.audio;
    a.addEventListener("timeupdate", () => this.patch({ positionMs: a.currentTime * 1000, durationMs: (a.duration || 0) * 1000 }));
    a.addEventListener("playing", () => this.patch({ playing: true, loading: false }));
    a.addEventListener("pause", () => this.patch({ playing: false }));
    a.addEventListener("waiting", () => this.patch({ loading: true }));
    a.addEventListener("ended", () => (this.state.repeatMode === "one" ? (a.currentTime = 0, a.play()) : this.next()));
    a.addEventListener("error", () => this.patch({ error: "couldn't play this song", loading: false }));
  }
  patch(p: Partial<PlayerState>) {
    this.state = { ...this.state, ...p, queueLength: this.queue.length, track: this.queue[this.state.index] ?? null };
    if ("index" in p) this.state.track = this.queue[p.index ?? -1] ?? null;
    this.listeners.forEach((l) => l(this.state));
  }
  async load(i: number, play = true) {
    const t = this.queue[i];
    if (!t) return;
    const token = ++this.token;
    this.patch({ index: i, loading: true, positionMs: 0, durationMs: t.durationMs ?? 0, error: null });
    try {
      const s = await api.stream(t.videoId);
      const wait = (s.availableAtMs ?? 0) - Date.now();
      if (wait > 0) await new Promise((r) => setTimeout(r, wait + 250));
      if (token !== this.token) return;
      this.audio.src = s.url;
      if (play) await this.audio.play();
    } catch (e) {
      if (token === this.token) this.patch({ error: String(e), loading: false });
    }
  }
  next() {
    if (this.state.index + 1 < this.queue.length) this.load(this.state.index + 1);
    else if (this.state.repeatMode === "all") this.load(0);
  }
}

let web: WebPlayer | null = null;
const webPlayer = () => (web ??= new WebPlayer());

// ---------------------------------------------------------------- API

export const player = {
  setQueue(tracks: Track[], index = 0, play = true) {
    if (!isNative) {
      const w = webPlayer();
      w.queue = [...tracks];
      return w.load(index, play);
    }
    return call("set_queue", { tracks: tracks.map(slim), index, play });
  },
  insert(tracks: Track[], next: boolean) {
    if (!isNative) {
      const w = webPlayer();
      if (!w.queue.length) return player.setQueue(tracks);
      w.queue.splice(next ? w.state.index + 1 : w.queue.length, 0, ...tracks);
      w.patch({});
      return Promise.resolve();
    }
    return call("insert", { tracks: tracks.map(slim), next });
  },
  toggle() {
    if (!isNative) {
      const a = webPlayer().audio;
      return a.paused ? a.play() : Promise.resolve(a.pause());
    }
    return call("toggle");
  },
  next: () => (isNative ? call("next") : Promise.resolve(webPlayer().next())),
  previous() {
    if (!isNative) {
      const w = webPlayer();
      if (w.audio.currentTime > 3 || w.state.index <= 0) w.audio.currentTime = 0;
      else w.load(w.state.index - 1);
      return Promise.resolve();
    }
    return call("previous");
  },
  seek(ms: number) {
    if (!isNative) {
      webPlayer().audio.currentTime = ms / 1000;
      return Promise.resolve();
    }
    return call("seek", { ms });
  },
  jump: (index: number) => (isNative ? call("jump", { index }) : webPlayer().load(index)),
  setRepeat(mode: PlayerState["repeatMode"]) {
    if (!isNative) return Promise.resolve(webPlayer().patch({ repeatMode: mode }));
    return call("set_repeat", { mode });
  },
  state: () => (isNative ? call<PlayerState>("get_state") : Promise.resolve(webPlayer().state)),
  /** The queue as the stand-in knows it (native keeps its own; the app mirrors it). */
  webQueue: () => (web ? web.queue : []),
  setLyrics: (videoId: string, data: { items: NativeLyricItem[]; approx: boolean; plain: string }) =>
    isNative ? call("set_lyrics", { videoId, ...data }) : Promise.resolve(),
  showNowPlaying: () => (isNative ? call("show_now_playing") : Promise.resolve()),
  haptic: (style: "light" | "medium" | "heavy" | "select" | "success" = "light") =>
    isNative ? call("haptic", { style }).catch(() => {}) : Promise.resolve(),
  async onState(cb: (s: PlayerState) => void): Promise<() => void> {
    if (!isNative) {
      const w = webPlayer();
      w.listeners.add(cb);
      return () => w.listeners.delete(cb);
    }
    const l = await addPluginListener("nocturn", "state", cb);
    return () => l.unregister();
  },
};

export const chrome = {
  setTabs: (tabs: TabSpec[], selected: string) => (isNative ? call("set_tabs", { tabs, selected }) : Promise.resolve()),
  select: (id: string) => (isNative ? call("select_tab", { id }) : Promise.resolve()),
  async onTab(cb: (id: string) => void): Promise<() => void> {
    if (!isNative) return () => {};
    const l = await addPluginListener("nocturn", "tab", (e: { id: string }) => cb(e.id));
    return () => l.unregister();
  },
  async onAction(cb: (kind: string, value: string | null) => void): Promise<() => void> {
    if (!isNative) return () => {};
    const l = await addPluginListener("nocturn", "action", (e: { kind: string; value: string | null }) => cb(e.kind, e.value));
    return () => l.unregister();
  },
};
