// Lyrics in the full player.
//
// Timing is read from the audio element on every frame (the player's own
// position updates only ~4×/s), shifted by the speaker delay the browser
// reports and by the user's per-song nudge. Only a change of line re-renders;
// the karaoke fill is driven by a CSS variable set directly on the element.

import { useEffect, useMemo, useRef, useState } from "react";
import { music, type Lyrics as LyricsData, type Track } from "../lib/api";
import { usePrefs } from "../shell/PrefsProvider";
import { activeIndex, buildItems, lyricTime, progress, wordProgress, type LyricItem } from "./lyricsTiming";
import { useMusic } from "./MusicProvider";

const STEP = 100; // nudge step (ms)

export function LyricsView({ track }: { track: Track }) {
  const m = useMusic();
  const { prefs } = usePrefs();
  const online = prefs.music.onlineLyrics;
  const reduce = prefs.accessibility.reduceMotion || document.body.classList.contains("reduce-motion");
  const [data, setData] = useState<LyricsData | null | undefined>(undefined);
  const [offset, setOffset] = useState(0);
  const [active, setActive] = useState(-1);
  const box = useRef<HTMLDivElement>(null);
  const rows = useRef<(HTMLDivElement | null)[]>([]);
  const userScrolledAt = useRef(0);
  const saveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    let live = true;
    setData(undefined);
    setActive(-1);
    music
      .lyrics(track.id, online)
      .then((d) => {
        if (!live) return;
        setData(d);
        setOffset(d?.offset_ms ?? 0);
      })
      .catch(() => live && setData(null));
    return () => {
      live = false;
    };
  }, [track.id, online]);

  const items: LyricItem[] = useMemo(() => (data?.synced.length ? buildItems(data.synced, data.words ?? [], track.duration_ms) : []), [data, track.duration_ms]);

  // The per-frame clock.
  const clock = useRef({ items, offset, active: -1 });
  clock.current.items = items;
  clock.current.offset = offset;
  useEffect(() => {
    if (!items.length) return;
    let raf = 0;
    const tick = () => {
      const c = clock.current;
      const t = lyricTime(m.timeMs(), m.latencyMs(), c.offset);
      const i = activeIndex(c.items, t);
      if (i !== c.active) {
        c.active = i;
        setActive(i);
      }
      const it = c.items[i];
      const el = i >= 0 ? rows.current[i] : null;
      if (it && el) {
        el.style.setProperty("--p", progress(it, t).toFixed(4));
        if (it.words.length) {
          const wp = wordProgress(it, t);
          el.querySelectorAll<HTMLElement>(".w").forEach((w, j) => w.style.setProperty("--p", (wp[j] ?? 0).toFixed(4)));
        }
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
    // m.timeMs / m.latencyMs are stable callbacks.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items]);

  // Keep the active line a little above the middle — unless the user is scrolling.
  useEffect(() => {
    const el = active >= 0 ? rows.current[active] : null;
    const b = box.current;
    if (!el || !b || Date.now() - userScrolledAt.current < 3500) return;
    b.scrollTo({ top: el.offsetTop - b.clientHeight * 0.38 + el.offsetHeight / 2, behavior: reduce ? "auto" : "smooth" });
  }, [active, reduce]);

  const nudge = (delta: number | null) => {
    const v = delta === null ? 0 : Math.max(-15000, Math.min(15000, offset + delta));
    setOffset(v);
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => music.setLyricsOffset(track.id, v).catch(() => {}), 500);
  };

  if (data === undefined) return <div className="small dim">Looking for lyrics…</div>;
  if (!data)
    return (
      <div className="small dim">
        {online
          ? "No lyrics found for this song — it has none of its own and LRCLIB doesn't have it (or couldn't be reached). A matching .lrc file next to the song always works."
          : 'No lyrics in this song. Turn on "Find lyrics online" in Customise › Music, or put a matching .lrc file next to it.'}
      </div>
    );

  if (!items.length) {
    return (
      <div className="lyrics plain">
        {data.text.split("\n").map((l, i) => <div key={i} className="lyric">{l || " "}</div>)}
        {data.source === "LRCLIB" && <div className="small dim lyric-credit">Lyrics from LRCLIB (not synced to this version)</div>}
      </div>
    );
  }

  const seekTo = (it: LyricItem) => m.seek(Math.max(0, it.start + m.latencyMs() - offset));
  return (
    <div className="lyrics-wrap">
      <div
        className={"lyrics synced" + (reduce ? " still" : "")}
        ref={box}
        onWheel={() => (userScrolledAt.current = Date.now())}
        onTouchMove={() => (userScrolledAt.current = Date.now())}
      >
        {items.map((it, i) => {
          const dist = active < 0 ? 99 : Math.abs(i - active);
          const cls = ["lyric", it.kind, i === active ? "on" : i < active ? "past" : "", dist === 1 ? "near" : dist === 2 ? "near2" : ""].filter(Boolean).join(" ");
          return (
            <div key={`${it.start}-${i}`} ref={(el) => { rows.current[i] = el; }} className={cls} onClick={() => seekTo(it)} title={it.kind === "line" ? "Play from here" : undefined}>
              {it.kind === "gap" ? (
                <span className="dots" aria-label="Instrumental">
                  <i style={{ ["--k" as string]: 0 }} />
                  <i style={{ ["--k" as string]: 1 }} />
                  <i style={{ ["--k" as string]: 2 }} />
                </span>
              ) : it.words.length ? (
                it.words.map(([wt, w], j) => (
                  <span key={`${wt}-${j}`} className="w">
                    {w}
                  </span>
                ))
              ) : (
                <span className="fill">{it.text}</span>
              )}
            </div>
          );
        })}
        <div className="lyrics-tail" />
      </div>
      <div className="lyrics-bar">
        <span className="small dim" title={data.approx ? "This song is a slowed or sped-up version; the timings were stretched from the original's." : undefined}>
          {data.approx ? "≈ Approximately synced" : data.source === "LRCLIB" ? "Lyrics from LRCLIB" : data.source === "lrc file" ? "From the .lrc file" : "From the file"}
        </span>
        <span className="lyrics-nudge">
          <button className="btn ghost sm" onClick={() => nudge(-STEP)} title="Lyrics are early — show them later">Later</button>
          <button className="btn ghost sm num" onClick={() => nudge(null)} disabled={!offset} title="Reset timing">
            {offset ? `${offset > 0 ? "+" : "−"}${(Math.abs(offset) / 1000).toFixed(1)} s` : "In time"}
          </button>
          <button className="btn ghost sm" onClick={() => nudge(STEP)} title="Lyrics are late — show them earlier">Earlier</button>
        </span>
      </div>
    </div>
  );
}
