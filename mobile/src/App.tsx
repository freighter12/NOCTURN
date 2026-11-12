import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, errorText, type Card, type Track } from "./api";
import { library, loadLibrary } from "./library";
import { chrome, isNative, player, settings, type PlayerState } from "./native";
import { AppContext, useApp, type AppCtx, type Route, type TabId } from "./state";
import { Art, Icon } from "./ui";
import Home from "./pages/Home";
import Search from "./pages/Search";
import LibraryPage, { DownloadsPage, LikedPage, MinePage, RecentPage, SavedPage } from "./pages/Library";
import SettingsPage from "./pages/Settings";
import StatsPage from "./pages/Stats";
import CollectionPage from "./pages/Collection";
import ArtistView from "./pages/Artist";
import { ActionSheet, ImportSheet, QueueSheet } from "./sheets";
import { buildLabel, updates, useUpdates } from "./update";
import { sync, useSync } from "./sync";
import { demoTracks, enableTrailer } from "./trailer";
import { buildItems } from "@shared/lyricsTiming";

const TABS: { id: TabId; title: string; symbol: string; search?: boolean }[] = [
  { id: "home", title: "Home", symbol: "house.fill" },
  { id: "library", title: "Library", symbol: "square.stack.fill" },
  { id: "search", title: "Search", symbol: "magnifyingglass", search: true },
];

const idle: PlayerState = {
  playing: false, loading: false, index: -1, positionMs: 0, durationMs: 0, queueLength: 0,
  track: null, repeatMode: "off", autoplay: true, error: null,
};

function shuffled<T>(a: T[]): T[] {
  const b = [...a];
  for (let i = b.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [b[i], b[j]] = [b[j], b[i]];
  }
  return b;
}

/** "Continue on iPhone": the PC was just playing something; carry on from the same spot. */
function HandoffBanner() {
  const app = useApp();
  const st = useSync();
  const h = st.handoff;
  if (!h?.track) return null;
  // already listening here and the PC is only paused: nothing to carry on
  if (app.player.playing && !h.playing) return null;
  const pc = library.get().sync?.pairing.name ?? "your PC";
  return (
    <div className="update-banner handoff-banner" role="status">
      <button
        className="update-main"
        onClick={async () => {
          const r = await sync.takeOver().catch((e) => {
            app.toast(String(e).replace(/^Error: /, ""));
            return null;
          });
          if (r?.track?.videoId) app.playList([r.track], 0, { positionMs: r.positionMs });
          else if (r) app.toast("That song couldn't be found here");
        }}
      >
        <Art src={h.track.artwork} size={40} radius={8} />
        <span>
          <b>Continue on iPhone</b>
          <small>
            {h.track.title} · {h.playing ? `playing on ${pc}` : `paused on ${pc}`}
          </small>
        </span>
      </button>
      <button className="update-x" aria-label="Hide" onClick={() => sync.dismissHandoff()}>
        <Icon name="close" size={16} />
      </button>
    </div>
  );
}

/** "Update available" across the top; tap for the steps in Settings › About. */
function UpdateBanner() {
  const app = useApp();
  const u = useUpdates();
  if (!u.available || u.dismissed || !u.latest) return null;
  return (
    <div className="update-banner" role="status">
      <button
        className="update-main"
        onClick={() => {
          updates.dismiss();
          if (app.tab !== "library") app.selectTab("library");
          setTimeout(() => app.push({ kind: "settings", section: "about" }), 60);
        }}
      >
        <span className="update-dot" />
        <span>
          <b>Update available</b>
          <small>{buildLabel(u.latest.build)} is out. Tap to see how to get it.</small>
        </span>
      </button>
      <button className="update-x" aria-label="Hide" onClick={() => updates.dismiss()}>
        <Icon name="close" size={16} />
      </button>
    </div>
  );
}

export default function App() {
  const [tab, setTab] = useState<TabId>("home");
  const [stacks, setStacks] = useState<Record<TabId, Route[]>>({ home: [{ kind: "root" }], search: [{ kind: "root" }], library: [{ kind: "root" }] });
  const [ps, setPs] = useState<PlayerState>(idle);
  const [queue, setQueue] = useState<Track[]>([]);
  const [actions, setActions] = useState<Track | null>(null);
  const [queueOpen, setQueueOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [toastText, setToastText] = useState<string | null>(null);
  const scrolls = useRef(new Map<string, number>());
  const known = useRef(new Map<string, Track>());
  const toastTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const stack = stacks[tab];
  const route = stack[stack.length - 1];
  const routeKey = `${tab}:${stack.length}:${JSON.stringify(route).slice(0, 80)}`;

  const aiOn = useRef(false);
  /** which way the last navigation went, for the page animation */
  const nav = useRef<"tab" | "push" | "back">("tab");
  // the first page waits until the start-up animation has finished
  const [introDone, setIntroDone] = useState(!isNative);
  useEffect(() => {
    if (!isNative) return;
    let done = false;
    const finish = () => {
      if (!done) {
        done = true;
        setIntroDone(true);
      }
    };
    const off = chrome.onAction((kind) => kind === "intro" && finish());
    settings
      .get()
      .then((s) => !s.introPlaying && finish())
      .catch(finish);
    const timer = setTimeout(finish, 3200);
    return () => {
      clearTimeout(timer);
      off.then((f) => f());
    };
  }, []);
  useEffect(() => updates.startupCheck(), []);
  useEffect(() => sync.startup(), []);
  // Trailer mode: the native timeline (Trailer.swift) says what to show next.
  useEffect(() => {
    const off = chrome.onAction((kind, value) => {
      if (kind !== "trailer" || !value) return;
      if (value === "setup") {
        enableTrailer();
        library.loadDemo(demoTracks);
      } else if (value === "home") {
        setStacks((s) => ({ ...s, home: [{ kind: "root" }] }));
        setTab("home");
        window.scrollTo({ top: 0 });
      } else if (value === "handoff") sync.showDemoHandoff(demoTracks[0]);
      else if (value === "hidehandoff") sync.dismissHandoff();
      else if (value === "library") {
        setStacks((s) => ({ ...s, library: [{ kind: "root" }] }));
        setTab("library");
        window.scrollTo({ top: 0 });
      } else if (value === "wrapped") {
        nav.current = "push";
        setStacks((s) => ({ ...s, library: [{ kind: "root" }, { kind: "stats" }] }));
        setTab("library");
        window.scrollTo({ top: 0 });
      } else if (value === "themes") {
        nav.current = "push";
        setStacks((s) => ({ ...s, library: [{ kind: "root" }, { kind: "settings" }, { kind: "settings", section: "appearance" }] }));
        setTab("library");
        window.scrollTo({ top: 0 });
      } else if (value.startsWith("scroll:")) window.scrollTo({ top: Number(value.slice(7)), behavior: "smooth" });
    });
    return () => {
      off.then((f) => f());
    };
  }, []);
  const toast = useCallback((text: string) => {
    setToastText(text);
    clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToastText(null), 2200);
  }, []);

  // remember where each page was scrolled so Back returns to the same spot
  const saveScroll = () => scrolls.current.set(routeKey, window.scrollY);
  useEffect(() => {
    const y = scrolls.current.get(routeKey) ?? 0;
    requestAnimationFrame(() => window.scrollTo(0, y));
  }, [routeKey]);

  const push = useCallback(
    (r: Route) => {
      saveScroll();
      nav.current = "push";
      setStacks((s) => ({ ...s, [tab]: [...s[tab], r] }));
      player.haptic("light");
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tab, routeKey],
  );
  const back = useCallback(() => {
    nav.current = "back";
    setStacks((s) => (s[tab].length > 1 ? { ...s, [tab]: s[tab].slice(0, -1) } : s));
  }, [tab]);

  const selectTab = useCallback(
    (t: TabId) => {
      saveScroll();
      nav.current = t === tab ? "back" : "tab";
      if (t === tab) {
        // tapping the current tab again goes back to its first page, like iOS
        setStacks((s) => ({ ...s, [t]: s[t].slice(0, 1) }));
        window.scrollTo({ top: 0, behavior: "smooth" });
      }
      setTab(t);
      chrome.select(t);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tab, routeKey],
  );

  // ---- player

  const remember = (ts: Track[]) => ts.forEach((t) => known.current.set(t.videoId, t));

  /** AI Shuffle: plan what plays after the current song (src-tauri/src/ai.rs). */
  const aiPlan = useCallback(
    (current: Track | null, upcoming: Track[], quiet = false) => {
      const lib = library.get();
      api
        .aiShuffle({ current, upcoming, recent: lib.recent, liked: lib.liked, skipped: ps.skipped ?? [], count: Math.max(25, upcoming.length) })
        .then((plan) => {
          remember(plan.upcoming);
          setQueue((q) => {
            const i = current ? q.findIndex((t) => t.videoId === current.videoId) : -1;
            return [...(i >= 0 ? q.slice(0, i + 1) : current ? [current] : []), ...plan.upcoming];
          });
          player.replaceUpcoming(plan.upcoming, plan.because);
          if (!quiet) toast(plan.because);
        })
        .catch((e) => {
          player.replaceUpcoming(upcoming, "");
          toast(errorText(e));
        });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [ps.skipped, toast],
  );

  const playList = useCallback<AppCtx["playList"]>(
    (tracks, index = 0, opts) => {
      const playable = tracks.filter(Boolean);
      if (!playable.length) return;
      let list = playable;
      let start = Math.min(index, list.length - 1);
      if (opts?.shuffle) {
        list = shuffled(playable);
        start = 0;
      }
      remember(list);
      setQueue(list);
      player.setQueue(list, start, true, opts?.positionMs).catch((e) => toast(errorText(e)));
      player.haptic("light");
      if (aiOn.current) aiPlan(list[start], list.slice(start + 1), true);
    },
    [toast, aiPlan],
  );
  const playNext = useCallback<AppCtx["playNext"]>(
    (ts) => {
      remember(ts);
      setQueue((q) => {
        const i = Math.max(0, ps.index + 1);
        return q.length ? [...q.slice(0, i), ...ts, ...q.slice(i)] : ts;
      });
      player.insert(ts, true).catch((e) => toast(errorText(e)));
      toast(ts.length === 1 ? "Playing next" : `${ts.length} songs playing next`);
    },
    [ps.index, toast],
  );
  const addToQueue = useCallback<AppCtx["addToQueue"]>(
    (ts) => {
      remember(ts);
      setQueue((q) => [...q, ...ts]);
      player.insert(ts, false).catch((e) => toast(errorText(e)));
      toast(ts.length === 1 ? "Added to queue" : `${ts.length} songs added to queue`);
    },
    [toast],
  );
  const startRadio = useCallback<AppCtx["startRadio"]>(
    (t) => {
      toast("Starting radio…");
      api
        .radio(t.videoId)
        .then((list) => {
          const songs = list.length ? list : [t];
          if (songs[0]?.videoId !== t.videoId) songs.unshift(t);
          playList(songs, 0);
        })
        .catch((e) => toast(errorText(e)));
    },
    [playList, toast],
  );

  const openCard = useCallback(
    (c: Card) => {
      if (c.kind === "song") {
        const t: Track = { videoId: c.id, title: c.title, artist: c.subtitle.split(" • ")[0] ?? "", artwork: c.artwork };
        playList([t], 0);
        return;
      }
      if (c.kind === "mine") push({ kind: "mine", id: c.id });
      else push(c.kind === "artist" ? { kind: "artist", id: c.id, seed: c } : { kind: "collection", id: c.id, seed: c });
    },
    [playList, push],
  );

  // player state from the native side
  useEffect(() => {
    let off: (() => void) | undefined;
    player.onState((s) => {
      if (s.track) {
        const full = known.current.get(s.track.videoId);
        if (full) s = { ...s, track: { ...s.track, ...full } };
      }
      setPs(s);
    }).then((f) => (off = f));
    player.state().then(setPs).catch(() => {});
    return () => off?.();
  }, []);

  // history + lyrics when the song changes
  const lastId = useRef<string | null>(null);
  useEffect(() => {
    const t = ps.track;
    if (!t || t.videoId === lastId.current) return;
    lastId.current = t.videoId;
    library.played(t);
    // Same timing model as the desktop app: lines, instrumental gaps, word timings.
    api
      .lyrics(t)
      .then((l) => {
        const duration = t.durationMs || ps.durationMs || 0;
        const items = l?.synced.length ? buildItems(l.synced, l.words ?? [], duration, true) : [];
        return player.setLyrics(t.videoId, {
          items: items.map((it) => ({ kind: it.kind, start: it.start, end: it.end, text: it.text, words: it.words.map(([wt, w]) => ({ t: wt, w })), estimated: it.estimated, sungEnd: it.sungEnd })),
          approx: !!l?.approx,
          plain: l?.text ?? "",
        });
      })
      .catch(() => player.setLyrics(t.videoId, { items: [], approx: false, plain: "" }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ps.track]);

  useEffect(() => {
    if (ps.error) toast(ps.error);
  }, [ps.error, toast]);

  useEffect(() => {
    aiOn.current = !!ps.aiShuffle;
  }, [ps.aiShuffle]);

  // native tab bar + Now Playing actions
  useEffect(() => {
    loadLibrary();
    chrome.setTabs(TABS, "home").catch(() => {});
    const offs: Promise<() => void>[] = [
      chrome.onTab((id) => {
        if (id === "home" || id === "search" || id === "library") {
          setTab((cur) => {
            nav.current = cur === id ? "back" : "tab";
            if (cur === id) setStacks((s) => ({ ...s, [id]: s[id].slice(0, 1) }));
            return id;
          });
        }
      }),
    ];
    return () => offs.forEach((p) => p.then((f) => f()));
  }, []);

  useEffect(() => {
    const p = chrome.onAction((kind, value) => {
      const t = ps.track ? (known.current.get(ps.track.videoId) ?? ps.track) : null;
      if (kind === "aiShuffle") {
        aiOn.current = value === "on";
        if (value === "on") {
          const i = ps.index;
          aiPlan(t, i >= 0 ? queue.slice(i + 1) : queue);
        }
      } else if (kind === "queue") setQueueOpen(true);
      else if (kind === "save" && t) toast(library.toggleLike(t) ? "Added to Liked Songs" : "Removed from Liked Songs");
      else if (kind === "radio" && t) startRadio(t);
      else if (kind === "artist" && t) {
        if (t.artistId) push({ kind: "artist", id: t.artistId });
        else api.search(value ?? t.artist).then((r) => r.artists[0] && push({ kind: "artist", id: r.artists[0].id, seed: r.artists[0] }));
      } else if (kind === "album" && t) {
        if (t.albumId) push({ kind: "collection", id: t.albumId });
        else api.search(`${t.artist} ${value ?? t.album ?? ""}`).then((r) => r.albums[0] && push({ kind: "collection", id: r.albums[0].id, seed: r.albums[0] }));
      }
    });
    return () => {
      p.then((f) => f());
    };
  }, [ps.track, ps.index, queue, aiPlan, push, startRadio, toast]);

  // swipe from the left edge to go back
  useEffect(() => {
    let sx = -1;
    let sy = 0;
    const down = (e: TouchEvent) => {
      const t = e.touches[0];
      sx = t.clientX < 24 ? t.clientX : -1;
      sy = t.clientY;
    };
    const up = (e: TouchEvent) => {
      if (sx < 0) return;
      const t = e.changedTouches[0];
      if (t.clientX - sx > 70 && Math.abs(t.clientY - sy) < 60) back();
      sx = -1;
    };
    window.addEventListener("touchstart", down, { passive: true });
    window.addEventListener("touchend", up, { passive: true });
    return () => {
      window.removeEventListener("touchstart", down);
      window.removeEventListener("touchend", up);
    };
  }, [back]);

  const ctx: AppCtx = useMemo(
    () => ({
      tab, route, depth: stack.length, push, back, selectTab,
      player: ps, queue, playList, playNext, addToQueue, startRadio, openCard,
      openActions: (t) => {
        player.haptic("medium");
        setActions(t);
      },
      openQueue: () => setQueueOpen(true),
      openImport: () => setImportOpen(true),
      toast,
      setAIShuffle: (on: boolean) => {
        aiOn.current = on;
        player.setAIShuffle(on).then(() => {
          if (!on) return;
          const t = ps.track ? (known.current.get(ps.track.videoId) ?? ps.track) : null;
          const i = ps.index;
          aiPlan(t, i >= 0 ? queue.slice(i + 1) : queue);
        });
      },
    }),
    [tab, route, stack.length, push, back, selectTab, ps, queue, playList, playNext, addToQueue, startRadio, openCard, toast, aiPlan],
  );

  let page: ReactNode;
  if (route.kind === "collection") page = <CollectionPage key={route.id} id={route.id} seed={route.seed} />;
  else if (route.kind === "artist") page = <ArtistView key={route.id} id={route.id} seed={route.seed} />;
  else if (route.kind === "mine") page = <MinePage key={route.id} id={route.id} />;
  else if (route.kind === "liked") page = <LikedPage />;
  else if (route.kind === "recent") page = <RecentPage />;
  else if (route.kind === "saved") page = <SavedPage what={route.what} />;
  else if (route.kind === "downloads") page = <DownloadsPage />;
  else if (route.kind === "stats") page = <StatsPage />;
  else if (route.kind === "settings") page = <SettingsPage key={route.section ?? "root"} section={route.section} />;
  else if (tab === "home") page = <Home />;
  else if (tab === "search") page = <Search />;
  else page = <LibraryPage />;

  return (
    <AppContext.Provider value={ctx}>
      <main className={`page go-${nav.current} ${introDone ? "" : "hold"} ${stack.length > 1 ? "pushed" : ""} ${isNative ? "native" : "web"}`} key={routeKey}>
        {page}
      </main>
      {!isNative && <WebChrome />}
      {actions && <ActionSheet track={actions} onClose={() => setActions(null)} />}
      {queueOpen && <QueueSheet onClose={() => setQueueOpen(false)} />}
      {importOpen && <ImportSheet onClose={() => setImportOpen(false)} />}
      {introDone && <UpdateBanner />}
      {introDone && <HandoffBanner />}
      {toastText && <div className="toast">{toastText}</div>}
    </AppContext.Provider>
  );
}

/** Stand-in tab bar + mini player when testing on a computer (the iPhone uses the native Liquid Glass ones). */
function WebChrome() {
  const app = useApp();
  const t = app.player.track;
  return (
    <div className="web-chrome">
      {t && (
        <div className="web-mini">
          <Art src={t.artwork} size={36} radius={7} />
          <div className="web-mini-text">
            <b>{t.title}</b>
            <span>{t.artist}</span>
          </div>
          <button onClick={() => player.toggle()} aria-label="Play or pause">
            <Icon name={app.player.playing ? "pause" : "play"} />
          </button>
          <button onClick={() => player.next()} aria-label="Next">
            <Icon name="next" />
          </button>
        </div>
      )}
      <nav className="web-tabs">
        {TABS.map((x) => (
          <button key={x.id} className={app.tab === x.id ? "on" : ""} onClick={() => app.selectTab(x.id)}>
            <Icon name={x.id === "home" ? "home" : x.id === "search" ? "search" : "stack"} />
            <span>{x.title}</span>
          </button>
        ))}
      </nav>
    </div>
  );
}
