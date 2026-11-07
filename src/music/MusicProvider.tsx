// Nocturn Music's playback engine. One audio element for the whole app, so
// music keeps playing on every page. Owns the queue, shuffle/repeat, play
// counting, Windows media-key integration and session restore.

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { artUrl, errorMessage, mediaUrl, music, musicStream, type Playlist, type Track } from "../lib/api";
import { useApp } from "../lib/app";
import { usePrefs } from "../shell/PrefsProvider";

export type Repeat = "off" | "all" | "one";

interface MusicCtx {
  tracks: Track[];
  byId: Map<number, Track>;
  playlists: Playlist[];
  loaded: boolean;
  reload: () => Promise<void>;
  reloadPlaylists: () => Promise<void>;
  queue: number[];
  index: number;
  current: Track | null;
  playing: boolean;
  position: number;
  duration: number;
  shuffle: boolean;
  repeat: Repeat;
  volume: number;
  play: (ids: number[], start?: number) => void;
  playNext: (ids: number[]) => void;
  addToQueue: (ids: number[]) => void;
  removeAt: (i: number) => void;
  moveInQueue: (from: number, to: number) => void;
  clearUpcoming: () => void;
  jumpTo: (i: number) => void;
  toggle: () => void;
  next: () => void;
  prev: () => void;
  seek: (ms: number) => void;
  setVolume: (v: number) => void;
  setShuffle: (on: boolean) => void;
  setRepeat: (r: Repeat) => void;
  setLiked: (id: number, liked: boolean) => void;
  /** Frequency data for the visualiser (null until playback starts). */
  analyser: AnalyserNode | null;
  /** Waiting for audio (a streamed song can take ~8 s to start). */
  buffering: boolean;
  /** Exact playback position right now (ms) — for per-frame lyrics. */
  timeMs: () => number;
  /** How far behind the speakers are (ms), when the browser knows (e.g. Bluetooth). */
  latencyMs: () => number;
}

const Ctx = createContext<MusicCtx | null>(null);

function shuffled<T>(xs: T[]): T[] {
  const a = [...xs];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

export function MusicProvider({ children }: { children: ReactNode }) {
  const { toast } = useApp();
  const { prefs, update } = usePrefs();
  const audio = useMemo(() => {
    const a = new Audio();
    a.crossOrigin = "anonymous";
    a.preload = "auto";
    return a;
  }, []);
  const [tracks, setTracks] = useState<Track[]>([]);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [queue, setQueue] = useState<number[]>([]);
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [buffering, setBuffering] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const [shuffle, setShuffleState] = useState(false);
  const [repeat, setRepeat] = useState<Repeat>("off");
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const original = useRef<number[] | null>(null);
  const counted = useRef(false);
  const restored = useRef(false);
  const audioCtx = useRef<AudioContext | null>(null);

  const byId = useMemo(() => new Map(tracks.map((t) => [t.id, t])), [tracks]);
  const current = byId.get(queue[index]) ?? null;
  const volume = prefs.music.volume;

  const reload = useCallback(async () => {
    try {
      setTracks(await music.tracks());
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't read your music library", detail: errorMessage(e) });
    } finally {
      setLoaded(true);
    }
  }, [toast]);
  const reloadPlaylists = useCallback(async () => {
    setPlaylists(await music.playlists().catch(() => []));
  }, []);

  useEffect(() => {
    reload();
    reloadPlaylists();
  }, [reload, reloadPlaylists]);

  // Restore the last session (paused) once the library is known.
  useEffect(() => {
    if (!loaded || restored.current) return;
    restored.current = true;
    music
      .session()
      .then((s) => {
        const q = s.queue.filter((id) => byId.has(id));
        if (!q.length) return;
        const i = Math.min(s.index, q.length - 1);
        setQueue(q);
        setIndex(i);
        setShuffleState(s.shuffle);
        setRepeat(s.repeat);
        audio.src = mediaUrl(q[i]);
        audio.currentTime = s.position_ms / 1000;
      })
      .catch(() => {});
  }, [loaded, byId, audio]);

  // Persist the session a moment after changes, and every 10 s while playing.
  useEffect(() => {
    if (!restored.current) return;
    const save = () => music.saveSession({ queue, index, position_ms: Math.round(audio.currentTime * 1000), shuffle, repeat }).catch(() => {});
    const t = window.setTimeout(save, 600);
    const iv = playing ? window.setInterval(save, 10_000) : undefined;
    return () => {
      window.clearTimeout(t);
      if (iv) window.clearInterval(iv);
    };
  }, [queue, index, shuffle, repeat, playing, audio]);

  useEffect(() => {
    audio.volume = volume;
  }, [audio, volume]);

  const ensureAnalyser = useCallback(() => {
    if (audioCtx.current) {
      if (audioCtx.current.state === "suspended") audioCtx.current.resume().catch(() => {});
      return;
    }
    try {
      const ctx = new AudioContext();
      const src = ctx.createMediaElementSource(audio);
      const an = ctx.createAnalyser();
      an.fftSize = 128;
      an.smoothingTimeConstant = 0.8;
      src.connect(an);
      an.connect(ctx.destination);
      audioCtx.current = ctx;
      setAnalyser(an);
    } catch {
      // Visualiser unavailable; playback still works.
    }
  }, [audio]);

  const load = useCallback(
    (id: number, autoplay: boolean) => {
      counted.current = false;
      audio.src = mediaUrl(id);
      setPosition(0);
      if (autoplay) {
        setBuffering(true);
        ensureAnalyser();
        audio.play().catch(() => {});
      }
    },
    [audio, ensureAnalyser],
  );

  const goto = useCallback(
    (i: number, autoplay = true) => {
      if (i < 0 || i >= queue.length) return;
      setIndex(i);
      load(queue[i], autoplay);
    },
    [queue, load],
  );

  const next = useCallback(() => {
    if (!queue.length) return;
    if (index + 1 < queue.length) goto(index + 1);
    else if (repeat === "all") goto(0);
    else {
      audio.pause();
      audio.currentTime = 0;
    }
  }, [queue, index, repeat, goto, audio]);

  const prev = useCallback(() => {
    // Restart the track if we're more than 3 s in, like most players.
    if (audio.currentTime > 3 || index === 0) {
      audio.currentTime = 0;
      return;
    }
    goto(index - 1);
  }, [audio, index, goto]);

  // Audio element events.
  useEffect(() => {
    const onTime = () => {
      setPosition(audio.currentTime * 1000);
      const t = byId.get(queue[index]);
      if (t && !counted.current && (audio.currentTime >= 30 || audio.currentTime * 1000 >= t.duration_ms / 2)) {
        counted.current = true;
        music.recordPlay(t.id, Math.round(audio.currentTime * 1000)).catch(() => {});
      }
    };
    const onMeta = () => setDuration((Number.isFinite(audio.duration) ? audio.duration : 0) * 1000);
    const onPlay = () => setPlaying(true);
    const onPause = () => {
      setPlaying(false);
      setBuffering(false);
    };
    const onWaiting = () => setBuffering(true);
    const onPlaying = () => setBuffering(false);
    const onEnded = () => {
      if (repeat === "one") {
        audio.currentTime = 0;
        audio.play().catch(() => {});
        counted.current = false;
      } else next();
    };
    const onError = () => {
      setBuffering(false);
      const t = byId.get(queue[index]);
      if (!t) return;
      const skip = () => {
        if (index + 1 < queue.length) goto(index + 1);
      };
      if (t.source === "youtube") {
        // The relay knows why (removed video, bot check, streaming off…).
        musicStream
          .error(t.id)
          .catch(() => null)
          .then((why) => {
            toast({ tone: "warning", title: `Couldn't stream ${t.title}`, detail: why ?? "YouTube didn't send the audio. Check your connection and try again." });
            skip();
          });
        return;
      }
      toast({ tone: "warning", title: `Couldn't play ${t.title}`, detail: "The file may have moved or been deleted. Rescan your music folders in Music › Sources." });
      skip();
    };
    audio.addEventListener("timeupdate", onTime);
    audio.addEventListener("loadedmetadata", onMeta);
    audio.addEventListener("play", onPlay);
    audio.addEventListener("pause", onPause);
    audio.addEventListener("ended", onEnded);
    audio.addEventListener("error", onError);
    audio.addEventListener("waiting", onWaiting);
    audio.addEventListener("playing", onPlaying);
    audio.addEventListener("canplay", onPlaying);
    return () => {
      audio.removeEventListener("waiting", onWaiting);
      audio.removeEventListener("playing", onPlaying);
      audio.removeEventListener("canplay", onPlaying);
      audio.removeEventListener("timeupdate", onTime);
      audio.removeEventListener("loadedmetadata", onMeta);
      audio.removeEventListener("play", onPlay);
      audio.removeEventListener("pause", onPause);
      audio.removeEventListener("ended", onEnded);
      audio.removeEventListener("error", onError);
    };
  }, [audio, byId, queue, index, repeat, next, goto, toast]);

  // Streamed songs take a few seconds to start: get the next two ready early.
  useEffect(() => {
    for (const k of [index + 1, index + 2]) {
      const id = queue[k];
      if (id != null && byId.get(id)?.source === "youtube") musicStream.prepare(id).catch(() => {});
    }
  }, [queue, index, byId]);

  // Windows media keys, the volume flyout and the lock screen.
  useEffect(() => {
    const ms = navigator.mediaSession;
    if (!ms) return;
    if (current) {
      ms.metadata = new MediaMetadata({ title: current.title, artist: current.artist, album: current.album, artwork: current.has_art ? [{ src: artUrl(current.id), sizes: "512x512" }] : [] });
    }
    ms.setActionHandler("play", () => audio.play());
    ms.setActionHandler("pause", () => audio.pause());
    ms.setActionHandler("nexttrack", () => next());
    ms.setActionHandler("previoustrack", () => prev());
    ms.setActionHandler("seekto", (d) => d.seekTime != null && (audio.currentTime = d.seekTime));
  }, [current, audio, next, prev]);

  const play = useCallback(
    (ids: number[], start = 0) => {
      const valid = ids.filter((id) => byId.has(id));
      if (!valid.length) return;
      const first = valid[Math.min(start, valid.length - 1)];
      let q = valid;
      let i = Math.min(start, valid.length - 1);
      if (shuffle) {
        original.current = valid;
        q = [first, ...shuffled(valid.filter((_, k) => k !== i))];
        i = 0;
      } else original.current = null;
      setQueue(q);
      setIndex(i);
      load(first, true);
    },
    [byId, shuffle, load],
  );

  const playNext = useCallback((ids: number[]) => setQueue((q) => (q.length ? [...q.slice(0, index + 1), ...ids, ...q.slice(index + 1)] : ids)), [index]);
  const addToQueue = useCallback((ids: number[]) => setQueue((q) => [...q, ...ids]), []);
  const removeAt = useCallback(
    (i: number) => {
      if (i === index) return; // remove the playing track by skipping it
      setQueue((q) => q.filter((_, k) => k !== i));
      if (i < index) setIndex((x) => x - 1);
    },
    [index],
  );
  const moveInQueue = useCallback(
    (from: number, to: number) => {
      if (from === to || from < 0 || to < 0) return;
      setQueue((q) => {
        const c = [...q];
        const [x] = c.splice(from, 1);
        c.splice(to, 0, x);
        return c;
      });
      setIndex((cur) => (cur === from ? to : from < cur && to >= cur ? cur - 1 : from > cur && to <= cur ? cur + 1 : cur));
    },
    [],
  );
  const clearUpcoming = useCallback(() => setQueue((q) => q.slice(0, index + 1)), [index]);
  const toggle = useCallback(() => {
    if (!current) return;
    if (audio.paused) {
      ensureAnalyser();
      audio.play().catch(() => {});
    } else audio.pause();
  }, [audio, current, ensureAnalyser]);
  const seek = useCallback((ms: number) => (audio.currentTime = Math.max(0, ms / 1000)), [audio]);
  const timeMs = useCallback(() => audio.currentTime * 1000, [audio]);
  const latencyMs = useCallback(() => {
    const c = audioCtx.current as (AudioContext & { outputLatency?: number }) | null;
    if (!c) return 0;
    const s = (c.outputLatency ?? 0) + (c.baseLatency ?? 0);
    return Number.isFinite(s) ? Math.min(500, Math.max(0, s * 1000)) : 0;
  }, []);
  const setVolume = useCallback((v: number) => update((p) => ({ ...p, music: { ...p.music, volume: v } })), [update]);
  const setShuffle = useCallback(
    (on: boolean) => {
      setShuffleState(on);
      const cur = queue[index];
      if (on) {
        original.current = queue;
        const rest = shuffled(queue.filter((_, k) => k !== index));
        setQueue([cur, ...rest]);
        setIndex(0);
      } else if (original.current) {
        const o = original.current;
        setQueue(o);
        setIndex(Math.max(0, o.indexOf(cur)));
        original.current = null;
      }
    },
    [queue, index],
  );
  const setLiked = useCallback(
    (id: number, liked: boolean) => {
      setTracks((ts) => ts.map((t) => (t.id === id ? { ...t, liked } : t)));
      music.setLike(id, liked).catch((e) => toast({ tone: "critical", title: "Couldn't save", detail: errorMessage(e) }));
    },
    [toast],
  );

  return (
    <Ctx.Provider
      value={{
        tracks, byId, playlists, loaded, reload, reloadPlaylists, queue, index, current, playing, position, duration, shuffle, repeat, volume,
        play, playNext, addToQueue, removeAt, moveInQueue, clearUpcoming, jumpTo: goto, toggle, next, prev, seek, setVolume, setShuffle, setRepeat, setLiked, analyser, buffering, timeMs, latencyMs,
      }}
    >
      {children}
    </Ctx.Provider>
  );
}

export function useMusic() {
  const c = useContext(Ctx);
  if (!c) throw new Error("useMusic outside MusicProvider");
  return c;
}

export function fmtTime(ms: number) {
  if (!Number.isFinite(ms) || ms < 0) return "0:00";
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}
