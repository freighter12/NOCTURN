// Nocturn Music's playback engine. Two audio "decks" for the whole app, so
// music keeps playing on every page and one song can run into the next:
// crossfade, gapless playback, and volume levelling through Web Audio.
// Owns the queue, shuffle/repeat, play counting, the sleep timer, Windows
// media-key integration and session restore.
//
//   deck A ─ level ─ fade ─┐
//                          ├─ mix ─ analyser ─ volume ─ speakers
//   deck B ─ level ─ fade ─┘
//
// "level" evens out loudness, "fade" is the crossfade, "volume" is the
// user's volume times any wind-down / sleep-timer fade.

import { listen } from "@tauri-apps/api/event";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { artUrl, errorMessage, mediaUrl, music, musicStream, type LoudnessMeasured, type Playlist, type Track } from "../lib/api";
import { useApp } from "../lib/app";
import { usePrefs } from "../shell/PrefsProvider";
import { fadeCurve, levelFor, overlapFor, typicalGain } from "./levels";

export type Repeat = "off" | "all" | "one";
/** A sleep timer: stop at a time, or when the current song ends. */
export type SleepTimer = { kind: "time"; endsAt: number } | { kind: "track" } | null;

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
  sleep: SleepTimer;
  /** Minutes from now, "track" (end of this song), or null (off). */
  setSleep: (s: number | "track" | null) => void;
  /** Extra volume factor 0–1 on top of the user's volume (wind-down). */
  setDim: (f: number) => void;
  /** Fade to silence over `seconds`, then pause. */
  fadeOutAndPause: (seconds: number) => void;
  /** Levelling applied to the current song (dB), when levelling is on. */
  levelDb: number | null;
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

interface Deck {
  el: HTMLAudioElement;
  /** Track loaded in this deck. */
  id: number | null;
  level: GainNode | null;
  fade: GainNode | null;
}

function newDeck(): Deck {
  const el = new Audio();
  el.crossOrigin = "anonymous";
  el.preload = "auto";
  return { el, id: null, level: null, fade: null };
}

/** Start preparing the next song this long before the current one ends. */
const PRELOAD_S = 25;
/** Gapless: start the next song this much before the end (play() takes a moment). */
const GAPLESS_LEAD_S = 0.06;
/** Skipping while a song plays: a short fade instead of a click. */
const SKIP_FADE_S = 0.12;
/** The sleep timer fades out over this long before it stops. */
const SLEEP_FADE_S = 20;

export function MusicProvider({ children }: { children: ReactNode }) {
  const { toast } = useApp();
  const { prefs, update } = usePrefs();
  const decks = useMemo(() => [newDeck(), newDeck()] as const, []);
  const active = useRef<0 | 1>(0);
  /** Bumped when the active deck changes, so effects re-attach. */
  const [deckGen, setDeckGen] = useState(0);
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
  const [sleep, setSleepState] = useState<SleepTimer>(null);
  const original = useRef<number[] | null>(null);
  const counted = useRef(false);
  const restored = useRef(false);
  const ctx = useRef<AudioContext | null>(null);
  const master = useRef<GainNode | null>(null);
  const dim = useRef(1);
  /** A fade-out (sleep timer / wind-down) in progress: its extra factor. */
  const fadeOut = useRef<{ timer: number; factor: number } | null>(null);
  /** The previous song still fading out after a crossfade. */
  const outgoing = useRef<{ deck: Deck; timer: number } | null>(null);
  const transition = useRef<number | null>(null);

  const byId = useMemo(() => new Map(tracks.map((t) => [t.id, t])), [tracks]);
  const current = byId.get(queue[index]) ?? null;
  const volume = prefs.music.volume;
  const mp = prefs.music;
  const fallbackDb = useMemo(() => typicalGain(tracks), [tracks]);

  const deck = useCallback(() => decks[active.current], [decks]);
  const idle = useCallback(() => decks[active.current === 0 ? 1 : 0], [decks]);

  // Latest values for timers and event handlers.
  const live = useRef({ queue, index, repeat, byId, mp, sleep, volume, fallbackDb });
  live.current = { queue, index, repeat, byId, mp, sleep, volume, fallbackDb };

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

  // ---------------------------------------------------------- the graph

  /** Output volume: the user's volume × wind-down dim × any fade-out. */
  const outVolume = useCallback(() => live.current.volume * dim.current * (fadeOut.current?.factor ?? 1), []);

  const applyVolume = useCallback(
    (ramp = 0.08) => {
      const v = outVolume();
      if (master.current && ctx.current) {
        const g = master.current.gain;
        const now = ctx.current.currentTime;
        g.cancelScheduledValues(now);
        g.setTargetAtTime(v, now, ramp / 3);
      } else {
        // No Web Audio: the element's own volume (levelling included).
        for (const d of decks) {
          const t = d.id != null ? live.current.byId.get(d.id) : null;
          d.el.volume = Math.min(1, v * levelFor(t, live.current.mp.normalize, live.current.fallbackDb));
        }
      }
    },
    [decks, outVolume],
  );

  /** Set a deck's levelling gain for its track (ramped, so a late measurement doesn't jump). */
  const applyLevel = useCallback(
    (d: Deck, ramp = 1.5) => {
      const t = d.id != null ? live.current.byId.get(d.id) : null;
      const g = levelFor(t, live.current.mp.normalize, live.current.fallbackDb);
      if (d.level && ctx.current) {
        const now = ctx.current.currentTime;
        d.level.gain.cancelScheduledValues(now);
        if (ramp <= 0) d.level.gain.setValueAtTime(g, now);
        else d.level.gain.setTargetAtTime(g, now, ramp / 3);
      } else applyVolume(0);
    },
    [applyVolume],
  );

  const setFade = (d: Deck, v: number) => {
    if (d.fade && ctx.current) {
      d.fade.gain.cancelScheduledValues(ctx.current.currentTime);
      d.fade.gain.setValueAtTime(v, ctx.current.currentTime);
    }
  };

  const ensureGraph = useCallback(() => {
    if (ctx.current) {
      if (ctx.current.state === "suspended") ctx.current.resume().catch(() => {});
      return;
    }
    try {
      const c = new AudioContext();
      const mix = c.createGain();
      const an = c.createAnalyser();
      an.fftSize = 128;
      an.smoothingTimeConstant = 0.8;
      const out = c.createGain();
      for (const d of decks) {
        const src = c.createMediaElementSource(d.el);
        d.level = c.createGain();
        d.fade = c.createGain();
        src.connect(d.level).connect(d.fade).connect(mix);
        d.el.volume = 1;
      }
      // The visualiser sees the music before the volume, so it moves at any volume.
      mix.connect(an).connect(out).connect(c.destination);
      ctx.current = c;
      master.current = out;
      out.gain.value = outVolume();
      for (const d of decks) applyLevel(d, 0);
      setAnalyser(an);
    } catch {
      // Web Audio unavailable: plain playback still works (no crossfade).
    }
  }, [decks, outVolume, applyLevel]);

  useEffect(() => applyVolume(), [volume, applyVolume]);
  // Levelling switched, a measurement arrived, or the library's typical level moved.
  useEffect(() => {
    for (const d of decks) applyLevel(d);
  }, [mp.normalize, byId, fallbackDb, decks, applyLevel]);

  // ------------------------------------------------------ measurements

  // Measurements arrive one at a time; apply them in batches.
  useEffect(() => {
    let pending: LoudnessMeasured[] = [];
    let timer = 0;
    const flush = () => {
      timer = 0;
      const got = new Map(pending.map((m) => [m.id, m]));
      pending = [];
      setTracks((ts) => ts.map((t) => (got.has(t.id) ? { ...t, loudness: got.get(t.id)!.lufs, peak: got.get(t.id)!.peak } : t)));
    };
    const un = listen<LoudnessMeasured>("music-loudness", (e) => {
      pending.push(e.payload);
      // The song that's about to play shouldn't wait.
      const soon = live.current.queue.slice(live.current.index, live.current.index + 2).includes(e.payload.id);
      if (soon) {
        window.clearTimeout(timer);
        flush();
      } else if (!timer) timer = window.setTimeout(flush, 2000);
    });
    return () => {
      window.clearTimeout(timer);
      un.then((f) => f());
    };
  }, []);

  // Measure the whole local library once, in the background.
  const backfilled = useRef(false);
  useEffect(() => {
    if (!loaded || !mp.normalize || backfilled.current || !tracks.length) return;
    backfilled.current = true;
    const t = window.setTimeout(() => music.loudnessBackfill().catch(() => {}), 15_000);
    return () => window.clearTimeout(t);
  }, [loaded, mp.normalize, tracks.length]);

  // The playing and next songs, if not measured yet: now.
  useEffect(() => {
    if (!mp.normalize) return;
    const ids = [queue[index], queue[index + 1]].filter((id): id is number => id != null && byId.get(id)?.loudness == null);
    if (ids.length) music.loudnessRequest(ids, true).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue, index, mp.normalize]);

  // ------------------------------------------------------------ loading

  /** Stop the outgoing deck of a crossfade right away. */
  const finishOutgoing = useCallback(() => {
    const o = outgoing.current;
    if (!o) return;
    window.clearTimeout(o.timer);
    o.deck.el.pause();
    setFade(o.deck, 1);
    outgoing.current = null;
  }, []);

  const cancelTransition = () => {
    if (transition.current != null) {
      window.clearTimeout(transition.current);
      transition.current = null;
    }
  };

  /** Fade a deck out over `secs` and pause it (it keeps its song loaded). */
  const retire = useCallback(
    (d: Deck, secs: number) => {
      finishOutgoing();
      if (d.el.paused) return;
      if (!d.fade || !ctx.current || secs <= 0) {
        d.el.pause();
        return;
      }
      const now = ctx.current.currentTime;
      const g = d.fade.gain;
      const from = g.value;
      g.cancelScheduledValues(now);
      g.setValueCurveAtTime(fadeCurve(64, false, from), now, secs);
      outgoing.current = {
        deck: d,
        timer: window.setTimeout(() => {
          d.el.pause();
          setFade(d, 1);
          outgoing.current = null;
        }, secs * 1000 + 80),
      };
    },
    [finishOutgoing],
  );

  /** Put `id` in the idle deck (unless it's already there). */
  const prime = useCallback(
    (id: number) => {
      const d = idle();
      if (d.id === id && d.el.src) return d;
      d.id = id;
      d.el.src = mediaUrl(id);
      d.el.load();
      setFade(d, 1);
      applyLevel(d, 0);
      return d;
    },
    [idle, applyLevel],
  );

  /** Make `d` the playing deck. */
  const promote = useCallback((d: Deck) => {
    active.current = decks[0] === d ? 0 : 1;
    counted.current = false;
    setDeckGen((g) => g + 1);
    setPosition(d.el.currentTime * 1000);
    setDuration((Number.isFinite(d.el.duration) ? d.el.duration : 0) * 1000);
  }, [decks]);

  const load = useCallback(
    (id: number, autoplay: boolean) => {
      cancelTransition();
      // A crossfade still running: its outgoing deck is the one we load into.
      finishOutgoing();
      const old = deck();
      const d = prime(id);
      if (d.el.currentTime > 0) d.el.currentTime = 0;
      if (autoplay) {
        setBuffering(true);
        ensureGraph();
        setFade(d, 1);
        d.el.play().catch(() => {});
        retire(old, SKIP_FADE_S);
      } else {
        old.el.pause();
      }
      promote(d);
    },
    [deck, prime, ensureGraph, retire, promote, finishOutgoing],
  );

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
        const d = deck();
        d.id = q[i];
        d.el.src = mediaUrl(q[i]);
        d.el.currentTime = s.position_ms / 1000;
        applyLevel(d, 0);
      })
      .catch(() => {});
  }, [loaded, byId, deck, applyLevel]);

  // Persist the session a moment after changes, and every 10 s while playing.
  useEffect(() => {
    if (!restored.current) return;
    const save = () => music.saveSession({ queue, index, position_ms: Math.round(deck().el.currentTime * 1000), shuffle, repeat }).catch(() => {});
    const t = window.setTimeout(save, 600);
    const iv = playing ? window.setInterval(save, 10_000) : undefined;
    return () => {
      window.clearTimeout(t);
      if (iv) window.clearInterval(iv);
    };
  }, [queue, index, shuffle, repeat, playing, deck]);

  const goto = useCallback(
    (i: number, autoplay = true) => {
      if (i < 0 || i >= queue.length) return;
      setIndex(i);
      load(queue[i], autoplay);
    },
    [queue, load],
  );

  /** The queue position after `i`, honouring repeat-all; null at the end. */
  const after = (i: number) => {
    const { queue, repeat } = live.current;
    if (i + 1 < queue.length) return i + 1;
    return repeat === "all" && queue.length ? 0 : null;
  };

  const stopAtEnd = useCallback(() => {
    const d = deck();
    d.el.pause();
    d.el.currentTime = 0;
  }, [deck]);

  const next = useCallback(() => {
    if (!queue.length) return;
    const n = after(index);
    if (n != null) goto(n);
    else stopAtEnd();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue, index, repeat, goto, stopAtEnd]);

  const prev = useCallback(() => {
    // Restart the track if we're more than 3 s in, like most players.
    const d = deck();
    if (d.el.currentTime > 3 || index === 0) {
      finishOutgoing();
      d.el.currentTime = 0;
      return;
    }
    goto(index - 1);
  }, [deck, index, goto, finishOutgoing]);

  // ------------------------------------------------------- transitions

  /** Hand over to the next song: crossfade over `overlap` s, or gapless (0). */
  const handover = useCallback(
    (ni: number, overlap: number) => {
      transition.current = null;
      finishOutgoing();
      const { queue } = live.current;
      const id = queue[ni];
      const old = deck();
      const d = prime(id);
      if (d.el.currentTime > 0) d.el.currentTime = 0;
      if (overlap > 0 && d.fade && ctx.current) {
        const now = ctx.current.currentTime;
        d.fade.gain.cancelScheduledValues(now);
        d.fade.gain.setValueCurveAtTime(fadeCurve(64, true), now, overlap);
      } else setFade(d, 1);
      d.el.play().catch(() => {});
      if (overlap > 0) retire(old, overlap);
      setIndex(ni);
      promote(d);
    },
    [deck, prime, retire, promote, finishOutgoing],
  );

  // Watch the end of the song: preload the next and start it on time.
  useEffect(() => {
    if (!playing) return;
    const iv = window.setInterval(() => {
      const d = deck();
      const { queue, index, byId, mp, sleep, repeat } = live.current;
      const dur = d.el.duration;
      if (d.el.paused || !Number.isFinite(dur) || dur <= 0 || transition.current != null) return;
      if (repeat === "one" || sleep?.kind === "track") return;
      const ni = after(index);
      if (ni == null) return;
      const rem = dur - d.el.currentTime;
      const nid = queue[ni];
      // (Not while the last song is still fading out of the other deck.)
      if (outgoing.current) return;
      if (rem < PRELOAD_S) prime(nid);
      const overlap = ctx.current ? overlapFor(byId.get(queue[index]), byId.get(nid), mp.crossfade, mp.keepAlbums) : 0;
      const lead = overlap > 0 ? overlap : mp.gapless ? GAPLESS_LEAD_S : -1;
      if (lead < 0 || rem > lead + 1.2) return;
      // Close to the moment: time it exactly.
      const nd = idle();
      if (nd.id !== nid || nd.el.readyState < 3) return; // not ready: fall back to "ended"
      const wait = Math.max(0, (rem - lead) * 1000);
      transition.current = window.setTimeout(() => handover(ni, overlap), wait);
    }, 250);
    return () => window.clearInterval(iv);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playing, deckGen, deck, idle, prime, handover]);

  // -------------------------------------------------------- deck events

  useEffect(() => {
    const d = deck();
    const audio = d.el;
    const mine = (e: Event) => e.target === audio;
    const onTime = (e: Event) => {
      if (!mine(e)) return;
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
      if (transition.current != null) return; // the handover is about to happen
      if (live.current.sleep?.kind === "track") {
        setSleepState(null);
        audio.currentTime = 0;
        return;
      }
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
    const events: [string, (e: Event) => void][] = [
      ["timeupdate", onTime], ["loadedmetadata", onMeta], ["durationchange", onMeta], ["play", onPlay], ["pause", onPause], ["ended", onEnded],
      ["error", onError], ["waiting", onWaiting], ["playing", onPlaying], ["canplay", onPlaying],
    ];
    for (const [n, f] of events) audio.addEventListener(n, f);
    // The deck may already be playing (a crossfade just handed over).
    setPlaying(!audio.paused);
    return () => {
      for (const [n, f] of events) audio.removeEventListener(n, f);
    };
  }, [deckGen, deck, byId, queue, index, repeat, next, goto, toast]);

  // Streamed songs take a few seconds to start: get the next two ready early.
  useEffect(() => {
    for (const k of [index + 1, index + 2]) {
      const id = queue[k];
      if (id != null && byId.get(id)?.source === "youtube") musicStream.prepare(id).catch(() => {});
    }
  }, [queue, index, byId]);

  // --------------------------------------------------------- sleep timer

  const fadeOutAndPause = useCallback(
    (seconds: number) => {
      if (fadeOut.current) window.clearInterval(fadeOut.current.timer);
      const d = deck();
      if (d.el.paused) {
        fadeOut.current = null;
        return;
      }
      const started = performance.now();
      const ms = Math.max(0.2, seconds) * 1000;
      const f = { timer: 0, factor: 1 };
      fadeOut.current = f;
      f.timer = window.setInterval(() => {
        const x = Math.min(1, (performance.now() - started) / ms);
        f.factor = Math.cos((x * Math.PI) / 2);
        applyVolume(0.25);
        if (x >= 1) {
          window.clearInterval(f.timer);
          finishOutgoing();
          deck().el.pause();
          fadeOut.current = null;
          // Back to full volume for next time, once paused.
          window.setTimeout(() => applyVolume(0), 150);
        }
      }, 200);
    },
    [deck, applyVolume, finishOutgoing],
  );

  // Cancel a fade-out if the user presses play again.
  useEffect(() => {
    if (playing && fadeOut.current && fadeOut.current.factor < 0.02) {
      window.clearInterval(fadeOut.current.timer);
      fadeOut.current = null;
      applyVolume(0);
    }
  }, [playing, applyVolume]);

  useEffect(() => {
    if (sleep?.kind !== "time") return;
    const tick = () => {
      const left = sleep.endsAt - Date.now();
      if (left <= SLEEP_FADE_S * 1000) {
        window.clearInterval(iv);
        setSleepState(null);
        fadeOutAndPause(Math.max(1, left / 1000));
      }
    };
    const iv = window.setInterval(tick, 1000);
    tick();
    return () => window.clearInterval(iv);
  }, [sleep, fadeOutAndPause]);

  const setSleep = useCallback((s: number | "track" | null) => {
    setSleepState(s == null ? null : s === "track" ? { kind: "track" } : { kind: "time", endsAt: Date.now() + Math.max(1, s) * 60_000 });
  }, []);

  const setDim = useCallback(
    (f: number) => {
      const v = Math.min(1, Math.max(0, f));
      if (Math.abs(v - dim.current) < 0.001) return;
      dim.current = v;
      applyVolume(2);
    },
    [applyVolume],
  );

  // -------------------------------------------------------- media keys

  // Windows media keys, the volume flyout and the lock screen.
  useEffect(() => {
    const ms = navigator.mediaSession;
    if (!ms) return;
    if (current) {
      ms.metadata = new MediaMetadata({ title: current.title, artist: current.artist, album: current.album, artwork: current.has_art ? [{ src: artUrl(current.id), sizes: "512x512" }] : [] });
    }
    ms.setActionHandler("play", () => {
      ensureGraph();
      deck().el.play();
    });
    ms.setActionHandler("pause", () => {
      finishOutgoing();
      deck().el.pause();
    });
    ms.setActionHandler("nexttrack", () => next());
    ms.setActionHandler("previoustrack", () => prev());
    ms.setActionHandler("seekto", (d) => d.seekTime != null && (deck().el.currentTime = d.seekTime));
  }, [current, deck, next, prev, ensureGraph, finishOutgoing]);

  // ------------------------------------------------------------- queue

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
    const d = deck();
    if (d.el.paused) {
      ensureGraph();
      d.el.play().catch(() => {});
    } else {
      cancelTransition();
      finishOutgoing();
      d.el.pause();
    }
  }, [deck, current, ensureGraph, finishOutgoing]);
  const seek = useCallback(
    (ms: number) => {
      cancelTransition();
      finishOutgoing();
      deck().el.currentTime = Math.max(0, ms / 1000);
    },
    [deck, finishOutgoing],
  );
  const timeMs = useCallback(() => deck().el.currentTime * 1000, [deck]);
  const latencyMs = useCallback(() => {
    const c = ctx.current as (AudioContext & { outputLatency?: number }) | null;
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

  // Queue edits can change what comes next: forget a planned handover.
  useEffect(() => cancelTransition(), [queue, repeat, mp.crossfade, mp.gapless]);

  // Development builds: let test scripts look at the decks.
  if (import.meta.env.DEV) (window as unknown as { __nocturnDecks: unknown }).__nocturnDecks = { decks, active, ctx, master };

  const levelDb = mp.normalize && current ? Math.round(20 * Math.log10(levelFor(current, true, fallbackDb)) * 10) / 10 : null;

  return (
    <Ctx.Provider
      value={{
        tracks, byId, playlists, loaded, reload, reloadPlaylists, queue, index, current, playing, position, duration, shuffle, repeat, volume,
        play, playNext, addToQueue, removeAt, moveInQueue, clearUpcoming, jumpTo: goto, toggle, next, prev, seek, setVolume, setShuffle, setRepeat, setLiked, analyser, buffering, timeMs, latencyMs,
        sleep, setSleep, setDim, fadeOutAndPause, levelDb,
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
