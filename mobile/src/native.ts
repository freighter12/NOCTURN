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
  aiShuffle?: boolean;
  skipped?: string[];
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

export type DownloadState = {
  tracks: Track[];
  pending: string[];
  active: string | null;
  progress: number;
  bytes: number;
  failed: string[];
};

export type ThemeName = "artwork" | "lunar" | "aurora" | "nebula" | "synthwave" | "ocean" | "snow" | "fireflies" | "black" | "photo";
export type Settings = {
  theme: ThemeName;
  photo: string | null;
  liveLyrics: boolean;
  /** background motion: 0.5 slow, 1 normal, 1.8 fast */
  speed: number;
  /** darkening behind text: 0 lighter, 1 normal, 2 darker */
  dim: number;
  /** background frozen */
  still: boolean;
  autoplay: boolean;
  aiShuffle: boolean;
  /** start-up animation */
  intro: IntroMode;
  /** the start-up animation is on screen right now */
  introPlaying: boolean;
  /** background smoothness: each theme's own pace, every frame (120 Hz), or half */
  refresh: Refresh;
  /** the screen's top refresh rate (120 on ProMotion iPhones) */
  maxFps: number;
  /** keep playing alongside other apps' sound */
  mixWithOthers: boolean;
  /** start again after another app or a call is done with the sound */
  autoResume: boolean;
  /** seconds one song fades into the next (0 = off, gapless) */
  crossfade: number;
  /** this build's version numbers (empty on the computer) */
  version: string;
  build: string;
};
export type IntroMode = "full" | "short" | "off";
export type Refresh = "auto" | "120" | "saver";
export const defaultSettings: Settings = {
  theme: "artwork", photo: null, liveLyrics: true, speed: 1, dim: 1, still: false, autoplay: true, aiShuffle: false,
  intro: "full", introPlaying: false, refresh: "auto", maxFps: 60, mixWithOthers: false, autoResume: true, crossfade: 0, version: "", build: "",
};

export type TabSpec = { id: string; title: string; symbol: string; search?: boolean };

let hapticsOn = true;
/** Settings › Playback › Haptics. */
export function setHapticsOn(on: boolean) {
  hapticsOn = on;
}

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
  replaceUpcoming(tracks: Track[], note: string) {
    if (!isNative) {
      const w = webPlayer();
      w.queue = [...w.queue.slice(0, w.state.index + 1), ...tracks];
      w.patch({});
      return Promise.resolve();
    }
    return call("replace_upcoming", { tracks: tracks.map(slim), note });
  },
  setAIShuffle: (on: boolean) => (isNative ? call("set_ai_shuffle", { on }) : Promise.resolve()),
  state: () => (isNative ? call<PlayerState>("get_state") : Promise.resolve(webPlayer().state)),
  /** The queue as the stand-in knows it (native keeps its own; the app mirrors it). */
  webQueue: () => (web ? web.queue : []),
  setLyrics: (videoId: string, data: { items: NativeLyricItem[]; approx: boolean; plain: string }) =>
    isNative ? call("set_lyrics", { videoId, ...data }) : Promise.resolve(),
  showNowPlaying: () => (isNative ? call("show_now_playing") : Promise.resolve()),
  haptic: (style: "light" | "medium" | "heavy" | "select" | "success" = "light") =>
    isNative && hapticsOn ? call("haptic", { style }).catch(() => {}) : Promise.resolve(),
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

// ---------------------------------------------------------------- downloads + settings

const noDownloads: DownloadState = { tracks: [], pending: [], active: null, progress: 0, bytes: 0, failed: [] };
const SETTINGS_KEY = "nocturn.settings";

function localSettings(): Settings {
  try {
    return { ...defaultSettings, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "{}") };
  } catch {
    return { ...defaultSettings };
  }
}

function saveLocal(patch: Partial<Settings>) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify({ ...localSettings(), ...patch }));
  } catch {
    /* storage full or blocked */
  }
}

export const downloads = {
  add: (tracks: Track[]) => (isNative ? call("download", { tracks: tracks.map(slim) }) : Promise.reject("Downloads work on the iPhone")),
  remove: (ids: string[]) => (isNative ? call("remove_downloads", { ids }) : Promise.resolve()),
  removeAll: () => (isNative ? call("remove_downloads", { all: true }) : Promise.resolve()),
  state: () => (isNative ? call<DownloadState>("get_downloads") : Promise.resolve(noDownloads)),
  async onChange(cb: (s: DownloadState) => void): Promise<() => void> {
    if (!isNative) return () => {};
    const l = await addPluginListener("nocturn", "downloads", cb);
    return () => l.unregister();
  },
};

export const settings = {
  get: () =>
    isNative ? call<Settings>("get_settings").then((s) => ({ ...defaultSettings, ...s })) : Promise.resolve(localSettings()),
  setLook(look: { speed?: number; dim?: number; still?: boolean; intro?: IntroMode; refresh?: Refresh }) {
    if (!isNative) return Promise.resolve(saveLocal(look));
    return call("set_look", look);
  },
  setCrossfade(seconds: number) {
    if (!isNative) return Promise.resolve(saveLocal({ crossfade: seconds }));
    return call("set_crossfade", { seconds });
  },
  setAudio(a: { mix?: boolean; resume?: boolean }) {
    if (!isNative) return Promise.resolve(saveLocal({ ...(a.mix != null && { mixWithOthers: a.mix }), ...(a.resume != null && { autoResume: a.resume }) }));
    return call("set_audio", a);
  },
  setAutoplay(on: boolean) {
    if (!isNative) return Promise.resolve(saveLocal({ autoplay: on }));
    return call("set_autoplay", { on });
  },
  setAIShuffle(on: boolean) {
    if (!isNative) return Promise.resolve(saveLocal({ aiShuffle: on }));
    return call("set_ai_shuffle", { on });
  },
  /** photo: base64 JPEG (no data: prefix), only when a new one was picked. */
  setTheme(theme: ThemeName, photo?: string) {
    if (!isNative) {
      const cur = localSettings();
      try {
        localStorage.setItem(SETTINGS_KEY, JSON.stringify({ ...cur, theme, photo: photo ?? cur.photo }));
      } catch {
        /* storage full or blocked */
      }
      return Promise.resolve();
    }
    return call("set_theme", { theme, photo: photo ?? null });
  },
  setLiveLyrics(on: boolean) {
    if (!isNative) {
      try {
        localStorage.setItem(SETTINGS_KEY, JSON.stringify({ ...localSettings(), liveLyrics: on }));
      } catch {
        /* ignore */
      }
      return Promise.resolve();
    }
    return call("set_live_lyrics", { on });
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
