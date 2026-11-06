// The persistent player in four modes (Full, Standard, Mini, Minimal) plus
// the queue drawer, lyrics and visualiser. Only shown once something has
// been queued.

import { useEffect, useRef, useState } from "react";
import { I } from "../components/Icons";
import { artUrl, errorMessage, music, type Lyrics as LyricsData, type Track } from "../lib/api";
import { useApp } from "../lib/app";
import { usePrefs } from "../shell/PrefsProvider";
import type { PlayerMode } from "../shell/prefs";
import { fmtTime, useMusic, type Repeat } from "./MusicProvider";

export function Art({ track, size, className = "" }: { track: Track | null | undefined; size: number; className?: string }) {
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [track?.id]);
  if (track?.has_art && !broken) {
    return <img className={`art ${className}`} src={artUrl(track.id)} width={size} height={size} alt="" onError={() => setBroken(true)} style={{ width: size, height: size }} />;
  }
  return (
    <span className={`art art-empty ${className}`} style={{ width: size, height: size }} aria-hidden>
      <I.music />
    </span>
  );
}

function Seek({ compact = false }: { compact?: boolean }) {
  const m = useMusic();
  const dur = m.duration || m.current?.duration_ms || 0;
  return (
    <div className={`seek ${compact ? "compact" : ""}`}>
      {!compact && <span className="num small dim">{fmtTime(m.position)}</span>}
      <input type="range" className="range seek-range" min={0} max={Math.max(1, dur)} step={500} value={Math.min(m.position, dur)} aria-label="Seek" onChange={(e) => m.seek(Number(e.target.value))} />
      {!compact && <span className="num small dim">{fmtTime(dur)}</span>}
    </div>
  );
}

function Controls({ big = false }: { big?: boolean }) {
  const m = useMusic();
  const nextRepeat: Record<Repeat, Repeat> = { off: "all", all: "one", one: "off" };
  return (
    <div className={`controls ${big ? "big" : ""}`}>
      <button className={`btn ghost icon ${m.shuffle ? "on" : ""}`} aria-label={m.shuffle ? "Shuffle on" : "Shuffle off"} aria-pressed={m.shuffle} onClick={() => m.setShuffle(!m.shuffle)}><I.shuffle /></button>
      <button className="btn ghost icon" aria-label="Previous" onClick={m.prev}><I.prev /></button>
      <button className="btn primary icon play" aria-label={m.playing ? "Pause" : "Play"} onClick={m.toggle}>{m.playing ? <I.pause /> : <I.play />}</button>
      <button className="btn ghost icon" aria-label="Next" onClick={m.next}><I.next /></button>
      <button className={`btn ghost icon ${m.repeat !== "off" ? "on" : ""}`} aria-label={`Repeat ${m.repeat}`} onClick={() => m.setRepeat(nextRepeat[m.repeat])}>
        <I.repeat />
        {m.repeat === "one" && <span className="badge-1">1</span>}
      </button>
    </div>
  );
}

function Volume() {
  const m = useMusic();
  return (
    <div className="row" style={{ gap: 6 }}>
      <I.volume style={{ width: 15, height: 15, color: "var(--ink-2)" }} />
      <input type="range" className="range" style={{ width: 96 }} min={0} max={1} step={0.02} value={m.volume} aria-label="Volume" onChange={(e) => m.setVolume(Number(e.target.value))} />
    </div>
  );
}

function Like({ track }: { track: Track }) {
  const m = useMusic();
  return (
    <button className={`btn ghost icon sm ${track.liked ? "on liked" : ""}`} aria-label={track.liked ? "Unlike" : "Like"} aria-pressed={track.liked} onClick={() => m.setLiked(track.id, !track.liked)}>
      <I.heart />
    </button>
  );
}

function ModeMenu({ mode, set }: { mode: PlayerMode; set: (m: PlayerMode) => void }) {
  const opts: [PlayerMode, string][] = [["full", "Full"], ["standard", "Standard"], ["mini", "Mini"], ["minimal", "Minimal"]];
  return (
    <select className="select mode-select" aria-label="Player mode" value={mode} onChange={(e) => set(e.target.value as PlayerMode)}>
      {opts.map(([v, l]) => <option key={v} value={v}>{l} player</option>)}
    </select>
  );
}

export function Visualiser({ height = 80 }: { height?: number }) {
  const { analyser, playing } = useMusic();
  const { prefs } = usePrefs();
  const ref = useRef<HTMLCanvasElement>(null);
  const reduce = prefs.accessibility.reduceMotion || document.body.classList.contains("reduce-motion");
  useEffect(() => {
    const c = ref.current;
    if (!c || !analyser || !prefs.music.visualiser) return;
    const g = c.getContext("2d")!;
    const data = new Uint8Array(analyser.frequencyBinCount);
    let raf = 0;
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--moon").trim() || "#dfe2e8";
    const draw = () => {
      const w = (c.width = c.clientWidth * devicePixelRatio);
      const h = (c.height = height * devicePixelRatio);
      analyser.getByteFrequencyData(data);
      g.clearRect(0, 0, w, h);
      const bars = 40;
      const bw = w / bars;
      g.fillStyle = accent;
      for (let i = 0; i < bars; i++) {
        const v = data[Math.floor((i / bars) * data.length * 0.8)] / 255;
        const bh = Math.max(2 * devicePixelRatio, v * h);
        g.globalAlpha = 0.35 + v * 0.65;
        g.fillRect(i * bw + bw * 0.2, h - bh, bw * 0.6, bh);
      }
      if (!reduce && playing) raf = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, [analyser, playing, reduce, height, prefs.music.visualiser]);
  if (!prefs.music.visualiser) return null;
  return <canvas ref={ref} className="visualiser" style={{ height }} aria-hidden />;
}

export function LyricsView({ track }: { track: Track }) {
  const { position } = useMusic();
  const [data, setData] = useState<LyricsData | null | undefined>(undefined);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    setData(undefined);
    music.lyrics(track.id).then(setData).catch(() => setData(null));
  }, [track.id]);
  // Last line whose timestamp has passed.
  let active = -1;
  if (data?.synced.length) for (let i = 0; i < data.synced.length && data.synced[i][0] <= position; i++) active = i;
  useEffect(() => {
    box.current?.querySelector(".lyric.on")?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [active]);
  if (data === undefined) return <div className="small dim">Looking for lyrics…</div>;
  if (!data) return <div className="small dim">No lyrics in this file. Nocturn reads lyrics embedded in the file or a matching .lrc file next to it.</div>;
  return (
    <div className="lyrics" ref={box}>
      {data.synced.length
        ? data.synced.map(([t, line], i) => <div key={`${t}-${i}`} className={`lyric ${i === active ? "on" : ""}`}>{line || "♪"}</div>)
        : data.text.split("\n").map((l, i) => <div key={i} className="lyric">{l}</div>)}
    </div>
  );
}

export function QueueDrawer({ onClose }: { onClose: () => void }) {
  const m = useMusic();
  const { toast } = useApp();
  const drag = useRef<number | null>(null);
  async function save() {
    try {
      const p = await music.savePlaylist({ name: `Queue ${new Date().toLocaleDateString()}`, description: "Saved from the queue", cover: {}, tracks: m.queue });
      await m.reloadPlaylists();
      toast({ tone: "good", title: `Saved as "${p.name}"` });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save the queue", detail: errorMessage(e) });
    }
  }
  return (
    <aside className="queue-drawer" aria-label="Queue">
      <div className="row between" style={{ padding: "14px 16px 8px" }}>
        <b>Queue</b>
        <div className="row" style={{ gap: 4 }}>
          <button className="btn ghost sm" onClick={save} disabled={!m.queue.length}>Save</button>
          <button className="btn ghost sm" onClick={m.clearUpcoming} disabled={m.index >= m.queue.length - 1}>Clear</button>
          <button className="btn ghost sm icon" aria-label="Close queue" onClick={onClose}><I.close /></button>
        </div>
      </div>
      <div className="queue-list">
        {m.queue.map((id, i) => {
          const t = m.byId.get(id);
          if (!t) return null;
          const now = i === m.index;
          return (
            <div
              key={`${id}-${i}`}
              className={`queue-item ${now ? "now" : ""} ${i < m.index ? "past" : ""}`}
              draggable={!now}
              onDragStart={() => (drag.current = i)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => {
                if (drag.current != null) m.moveInQueue(drag.current, i);
                drag.current = null;
              }}
            >
              <button className="queue-open" onClick={() => m.jumpTo(i)} aria-label={`Play ${t.title}`}>
                <Art track={t} size={34} />
                <span className="stack" style={{ gap: 0, minWidth: 0 }}>
                  <span className="truncate">{t.title}</span>
                  <span className="small dim truncate">{t.artist}</span>
                </span>
              </button>
              {now ? <span className="tag solid">Playing</span> : <button className="btn ghost sm icon" aria-label={`Remove ${t.title} from queue`} onClick={() => m.removeAt(i)}><I.close /></button>}
            </div>
          );
        })}
      </div>
    </aside>
  );
}

export function Player() {
  const m = useMusic();
  const { prefs, update } = usePrefs();
  const { navigate } = useApp();
  const [queueOpen, setQueueOpen] = useState(false);
  const [fullTab, setFullTab] = useState<"lyrics" | "queue">("lyrics");
  const mode = prefs.music.playerMode;
  const setMode = (v: PlayerMode) => update((p) => ({ ...p, music: { ...p.music, playerMode: v } }));
  const t = m.current;
  if (!t) return null;
  const info = (
    <div className="np-info">
      <span className="np-title truncate">{t.title}</span>
      <span className="small muted truncate">{t.artist}{t.album ? ` · ${t.album}` : ""}</span>
    </div>
  );

  if (mode === "full") {
    return (
      <div className="player-full" role="region" aria-label="Now playing">
        <div className="pf-top">
          <button className="btn ghost sm" onClick={() => navigate("music")}><I.music /> Music</button>
          <ModeMenu mode={mode} set={setMode} />
        </div>
        <div className="pf-body">
          <div className="pf-left">
            <Art track={t} size={340} className="pf-art" />
            <div className="row between" style={{ width: 340 }}>
              {info}
              <Like track={t} />
            </div>
            <Seek />
            <Controls big />
            <div className="row" style={{ justifyContent: "center" }}><Volume /></div>
            <Visualiser height={64} />
          </div>
          <div className="pf-right">
            <div className="seg" style={{ alignSelf: "flex-start" }}>
              <button className={fullTab === "lyrics" ? "on" : ""} onClick={() => setFullTab("lyrics")}>Lyrics</button>
              <button className={fullTab === "queue" ? "on" : ""} onClick={() => setFullTab("queue")}>Up next</button>
            </div>
            {fullTab === "lyrics" ? (prefs.music.showLyrics ? <LyricsView track={t} /> : <div className="small dim">Lyrics are turned off in Customise › Music.</div>) : <QueueDrawer onClose={() => setFullTab("lyrics")} />}
          </div>
        </div>
      </div>
    );
  }

  if (mode === "mini") {
    return (
      <div className="player-mini" role="region" aria-label="Now playing">
        <Art track={t} size={56} />
        <div className="stack" style={{ gap: 4, minWidth: 0, flex: 1 }}>
          {info}
          <Seek compact />
          <div className="row" style={{ gap: 2 }}>
            <button className="btn ghost sm icon" aria-label="Previous" onClick={m.prev}><I.prev /></button>
            <button className="btn primary sm icon" aria-label={m.playing ? "Pause" : "Play"} onClick={m.toggle}>{m.playing ? <I.pause /> : <I.play />}</button>
            <button className="btn ghost sm icon" aria-label="Next" onClick={m.next}><I.next /></button>
            <Like track={t} />
            <button className="btn ghost sm icon" aria-label="Expand player" style={{ marginLeft: "auto" }} onClick={() => setMode("standard")}>⤢</button>
          </div>
        </div>
      </div>
    );
  }

  if (mode === "minimal") {
    return (
      <div className="player-minimal" role="region" aria-label="Now playing">
        <button className="btn primary sm icon" aria-label={m.playing ? "Pause" : "Play"} onClick={m.toggle}>{m.playing ? <I.pause /> : <I.play />}</button>
        <span className="truncate small" style={{ maxWidth: 180 }} title={`${t.title} — ${t.artist}`}>{t.title}</span>
        <button className="btn ghost sm icon" aria-label="Next" onClick={m.next}><I.next /></button>
        <button className="btn ghost sm icon" aria-label="Expand player" onClick={() => setMode("standard")}>⤢</button>
      </div>
    );
  }

  return (
    <>
      <div className="player-bar" role="region" aria-label="Now playing">
        <div className="pb-left">
          <button className="art-btn" aria-label="Open full player" onClick={() => setMode("full")}><Art track={t} size={46} /></button>
          {info}
          <Like track={t} />
        </div>
        <div className="pb-center">
          <Controls />
          <Seek />
        </div>
        <div className="pb-right">
          <Volume />
          <button className={`btn ghost icon sm ${queueOpen ? "on" : ""}`} aria-label="Queue" aria-pressed={queueOpen} onClick={() => setQueueOpen((v) => !v)}><I.queue /></button>
          <ModeMenu mode={mode} set={setMode} />
        </div>
      </div>
      {queueOpen && <QueueDrawer onClose={() => setQueueOpen(false)} />}
    </>
  );
}
