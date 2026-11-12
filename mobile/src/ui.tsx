import { useEffect, useRef, useState, type MouseEvent as RMouseEvent, type PointerEvent as RPointerEvent, type ReactNode } from "react";
import { fmtTime, type Card, type Track } from "./api";
import { library, useLibrary } from "./library";
import { offline, useDownloads } from "./downloads";
import { useApp } from "./state";

// ---------------------------------------------------------------- icons (SF Symbols-like)

const paths: Record<string, string> = {
  play: "M8 5.6v12.8c0 .8.9 1.3 1.6.9l10-6.4c.6-.4.6-1.4 0-1.8l-10-6.4C8.9 4.3 8 4.8 8 5.6z",
  pause: "M7 5h3.2v14H7zM13.8 5H17v14h-3.2z",
  next: "M4 6.2v11.6c0 .7.8 1.1 1.4.7L13 13v4.8c0 .7.8 1.1 1.4.7l8-5.8c.5-.4.5-1.1 0-1.4l-8-5.8c-.6-.4-1.4 0-1.4.7V11L5.4 5.5C4.8 5.1 4 5.5 4 6.2z",
  shuffle: "M16 4l4 3.5-4 3.5V8.6h-1.6c-1.2 0-2.3.6-3 1.6l-4.6 6.5c-1 1.5-2.7 2.3-4.5 2.3H2v-2.2h.3c1.1 0 2.1-.5 2.7-1.4l4.6-6.5c1.1-1.6 2.9-2.5 4.8-2.5H16zM2 6.8h.3c1.8 0 3.5.9 4.5 2.3l.8 1.1-1.3 1.9-1.3-1.8C4.4 9.5 3.4 9 2.3 9H2zm14 6.2l4 3.5-4 3.5v-2.3h-1.6c-1.9 0-3.7-.9-4.8-2.5l-.6-.8 1.3-1.9 1.1 1.5c.7 1 1.8 1.6 3 1.6H16z",
  more: "M5 10.2a1.8 1.8 0 110 3.6 1.8 1.8 0 010-3.6zm7 0a1.8 1.8 0 110 3.6 1.8 1.8 0 010-3.6zm7 0a1.8 1.8 0 110 3.6 1.8 1.8 0 010-3.6z",
  back: "M15.4 4.6a1.2 1.2 0 010 1.7L9.7 12l5.7 5.7a1.2 1.2 0 11-1.7 1.7l-6.5-6.5a1.2 1.2 0 010-1.8l6.5-6.5a1.2 1.2 0 011.7 0z",
  heart: "M12 20.3l-1.3-1.2C5.9 14.8 3 12.1 3 8.7 3 6 5.1 4 7.7 4c1.6 0 3.2.8 4.3 2 1.1-1.2 2.7-2 4.3-2C18.9 4 21 6 21 8.7c0 3.4-2.9 6.1-7.7 10.4L12 20.3z",
  heartO: "M16.3 4c-1.6 0-3.2.8-4.3 2-1.1-1.2-2.7-2-4.3-2C5.1 4 3 6 3 8.7c0 3.4 2.9 6.1 7.7 10.4l1.3 1.2 1.3-1.2C18.1 14.8 21 12.1 21 8.7 21 6 18.9 4 16.3 4zm-4.2 13.8l-.1.1-.1-.1C7.6 14 5 11.6 5 8.7 5 7.1 6.2 6 7.7 6c1.2 0 2.4.8 2.8 1.9h1c.4-1.1 1.6-1.9 2.8-1.9 1.5 0 2.7 1.1 2.7 2.7 0 2.9-2.6 5.3-6.9 9.1z",
  plus: "M12 4.5c.6 0 1.1.5 1.1 1.1v5.3h5.3a1.1 1.1 0 010 2.2h-5.3v5.3a1.1 1.1 0 01-2.2 0v-5.3H5.6a1.1 1.1 0 010-2.2h5.3V5.6c0-.6.5-1.1 1.1-1.1z",
  check: "M9.6 16.6L5.3 12.3a1.1 1.1 0 00-1.6 1.6l5.1 5.1c.4.4 1.1.4 1.6 0L20.3 9a1.1 1.1 0 00-1.6-1.6z",
  search: "M10.5 3a7.5 7.5 0 015.9 12.1l4.3 4.3a1.1 1.1 0 01-1.6 1.6l-4.3-4.3A7.5 7.5 0 1110.5 3zm0 2.2a5.3 5.3 0 100 10.6 5.3 5.3 0 000-10.6z",
  close: "M6.3 5.2L12 10.9l5.7-5.7a.8.8 0 011.1 1.1L13.1 12l5.7 5.7a.8.8 0 01-1.1 1.1L12 13.1l-5.7 5.7a.8.8 0 01-1.1-1.1l5.7-5.7-5.7-5.7a.8.8 0 011.1-1.1z",
  radio: "M12 9.8a2.2 2.2 0 110 4.4 2.2 2.2 0 010-4.4zM7.4 7.4a1 1 0 010 1.4 4.5 4.5 0 000 6.4 1 1 0 01-1.4 1.4 6.5 6.5 0 010-9.2 1 1 0 011.4 0zm9.2 0a1 1 0 011.4 0 6.5 6.5 0 010 9.2 1 1 0 01-1.4-1.4 4.5 4.5 0 000-6.4 1 1 0 010-1.4zM4.6 4.6a1 1 0 010 1.4 8.5 8.5 0 000 12 1 1 0 01-1.4 1.4 10.5 10.5 0 010-14.8 1 1 0 011.4 0zm14.8 0a1 1 0 011.4 0 10.5 10.5 0 010 14.8 1 1 0 01-1.4-1.4 8.5 8.5 0 000-12 1 1 0 010-1.4z",
  queueNext: "M3 6h12v2H3zm0 5h12v2H3zm0 5h8v2H3zm13 0l5 3-5 3z",
  queueEnd: "M3 6h12v2H3zm0 5h12v2H3zm0 5h8v2H3zm11-1h2v3h3v2h-3v3h-2v-3h-3v-2h3z",
  album: "M12 3a9 9 0 110 18 9 9 0 010-18zm0 6.5a2.5 2.5 0 100 5 2.5 2.5 0 000-5z",
  mic: "M12 3a3 3 0 013 3v6a3 3 0 01-6 0V6a3 3 0 013-3zm-6 9h2a4 4 0 008 0h2a6 6 0 01-5 5.9V21h-2v-3.1A6 6 0 016 12z",
  note: "M18 3v11.5a3.5 3.5 0 11-2-3.2V7l-7 1.6v7.9a3.5 3.5 0 11-2-3.2V5.5z",
  home: "M12 3.2l8.5 7V20a1 1 0 01-1 1h-5v-6h-5v6h-5a1 1 0 01-1-1v-9.8z",
  stack: "M4 8h16v12H4zM6 5h12v2H6zM8 2h8v2H8z",
  download: "M12 3a1 1 0 011 1v9.6l3.3-3.3a1 1 0 111.4 1.4l-5 5a1 1 0 01-1.4 0l-5-5a1 1 0 111.4-1.4l3.3 3.3V4a1 1 0 011-1zM5 19h14a1 1 0 010 2H5a1 1 0 010-2z",
  settings: "M4 6h9.2a2.6 2.6 0 014.6 0H20v2h-2.2a2.6 2.6 0 01-4.6 0H4zm0 5h2.2a2.6 2.6 0 014.6 0H20v2h-9.2a2.6 2.6 0 01-4.6 0H4zm0 5h9.2a2.6 2.6 0 014.6 0H20v2h-2.2a2.6 2.6 0 01-4.6 0H4z",
  trash: "M9 3h6l1 2h4v2H4V5h4zM6 9h12l-1 11a1 1 0 01-1 1H8a1 1 0 01-1-1z",
  palette: "M12 3a9 9 0 100 18c1 0 1.7-.8 1.7-1.7 0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2 0-.9.8-1.7 1.7-1.7H16a5 5 0 005-5C21 6.6 17 3 12 3zM6.5 13a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm3-4a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm5 0a1.5 1.5 0 110-3 1.5 1.5 0 010 3zm3 4a1.5 1.5 0 110-3 1.5 1.5 0 010 3z",
  info: "M12 3a9 9 0 110 18 9 9 0 010-18zm0 7.5a1 1 0 00-1 1V16a1 1 0 002 0v-4.5a1 1 0 00-1-1zm0-3.3a1.2 1.2 0 100 2.4 1.2 1.2 0 000-2.4z",
  chart: "M4 20h16v-2H4zM5 16h3V9H5zm5.5 0h3V4h-3zM16 16h3v-5h-3z",
  pc: "M3 5a1 1 0 011-1h16a1 1 0 011 1v10a1 1 0 01-1 1h-6v2h3v2H7v-2h3v-2H4a1 1 0 01-1-1zm2 1v8h14V6z",
  clock: "M12 3a9 9 0 110 18 9 9 0 010-18zm0 2a7 7 0 100 14 7 7 0 000-14zm0 2a1 1 0 011 1v3.6l2.5 1.5a1 1 0 01-1 1.7l-3-1.8A1 1 0 0111 12V8a1 1 0 011-1z",
  sparkle: "M12 2l1.8 5.6L19.5 9.5l-5.7 1.9L12 17l-1.8-5.6L4.5 9.5l5.7-1.9zM19 15l.9 2.6 2.6.9-2.6.9L19 22l-.9-2.6-2.6-.9 2.6-.9z",
  greet: "M4 5h16a1 1 0 011 1v10a1 1 0 01-1 1h-9l-4.3 3.4c-.3.3-.7 0-.7-.4V17H4a1 1 0 01-1-1V6a1 1 0 011-1zm3 4.2a1.3 1.3 0 100 2.6 1.3 1.3 0 000-2.6zm5 0a1.3 1.3 0 100 2.6 1.3 1.3 0 000-2.6zm5 0a1.3 1.3 0 100 2.6 1.3 1.3 0 000-2.6z",
};

export function Icon({ name, size = 22, className }: { name: keyof typeof paths | string; size?: number; className?: string }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} className={className} aria-hidden="true">
      <path d={paths[name] ?? paths.note} fill="currentColor" />
    </svg>
  );
}

// ---------------------------------------------------------------- artwork

export function Art({ src, size, round, radius = 8, className }: { src?: string | null; size?: number; round?: boolean; radius?: number; className?: string }) {
  // 0 = first try, 1 = retrying once (image hosts sometimes fail a request), 2 = give up
  const [attempt, setAttempt] = useState(0);
  useEffect(() => setAttempt(0), [src]);
  const style = size ? { width: size, height: size, borderRadius: round ? "50%" : radius } : { borderRadius: round ? "50%" : radius };
  const url = src && attempt === 1 ? `${src}${src.includes("?") ? "&" : "?"}r=1` : src;
  return (
    <div className={`art ${className ?? ""}`} style={style}>
      {url && attempt < 2 ? (
        <img key={attempt} src={url} alt="" loading="lazy" decoding="async" onError={() => setTimeout(() => setAttempt((a) => a + 1), attempt ? 0 : 600)} />
      ) : (
        <Icon name="note" size={size ? Math.max(14, size * 0.36) : 28} />
      )}
    </div>
  );
}

// ---------------------------------------------------------------- song rows

export function TrackRow({ track, list, index, number, onPlay }: { track: Track; list: Track[]; index: number; number?: number; onPlay?: () => void }) {
  const app = useApp();
  const current = app.player.track?.videoId === track.videoId;
  useDownloads();
  const saved = offline.has(track.videoId);
  const { fired, ...press } = useLongPress(() => app.openActions(track));
  return (
    <div className={`row ${current ? "current" : ""}`} {...press}>
      <button className="row-main" onClick={() => (fired() ? undefined : onPlay ? onPlay() : app.playList(list, index))}>
        {number !== undefined ? (
          <span className="row-num">{current && app.player.playing ? <Bars /> : number}</span>
        ) : (
          <Art src={track.artwork} size={48} radius={6} className={track.video ? "wide" : ""} />
        )}
        <span className="row-text">
          <span className="row-title">{track.title}</span>
          <span className="row-sub">
            {saved && <Icon name="download" size={13} className="dl-mark" />}
            {track.explicit && <span className="explicit">E</span>}
            {track.artist}
            {track.album && number === undefined ? ` · ${track.album}` : ""}
          </span>
        </span>
      </button>
      <button className="row-more" aria-label="More" onClick={() => app.openActions(track)}>
        <Icon name="more" size={20} />
      </button>
    </div>
  );
}

export function Bars() {
  return (
    <span className="bars" aria-label="Playing">
      <i />
      <i />
      <i />
    </span>
  );
}

// ---------------------------------------------------------------- tiles (albums, playlists, artists)

export function Tile({ card, wide }: { card: Card; wide?: boolean }) {
  const app = useApp();
  const round = card.kind === "artist";
  return (
    <button className={`tile ${round ? "round" : ""} ${wide ? "wide" : ""}`} onClick={() => app.openCard(card)}>
      <Art src={card.artwork} round={round} radius={10} />
      <span className="tile-title">{card.title}</span>
      {card.subtitle && <span className="tile-sub">{card.subtitle}</span>}
    </button>
  );
}

export function Shelf({ title, children, onMore }: { title: string; children: ReactNode; onMore?: () => void }) {
  return (
    <section className="shelf">
      <header className="shelf-head">
        <h2>{title}</h2>
        {onMore && (
          <button className="link" onClick={onMore}>
            See All
          </button>
        )}
      </header>
      {children}
    </section>
  );
}

export function Scroller({ children }: { children: ReactNode }) {
  return <div className="scroller">{children}</div>;
}

// ---------------------------------------------------------------- page chrome

export function PageHeader({ title, onBack, right, large = true }: { title: string; onBack?: () => void; right?: ReactNode; large?: boolean }) {
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const on = () => setScrolled(window.scrollY > (large ? 44 : 8));
    on();
    window.addEventListener("scroll", on, { passive: true });
    return () => window.removeEventListener("scroll", on);
  }, [large]);
  return (
    <>
      <div className={`navbar ${scrolled ? "scrolled" : ""}`}>
        <div className="navbar-side">
          {onBack && (
            <button className="nav-back" onClick={onBack} aria-label="Back">
              <Icon name="back" size={22} />
            </button>
          )}
        </div>
        <div className={`navbar-title ${scrolled || !large ? "show" : ""}`}>{title}</div>
        <div className="navbar-side right">{right}</div>
      </div>
      {large && <h1 className="large-title">{title}</h1>}
    </>
  );
}

export function Loading({ label = "Loading" }: { label?: string }) {
  return (
    <div className="loading" role="status">
      <span className="spinner" />
      <span>{label}…</span>
    </div>
  );
}

export function ErrorBox({ text, retry }: { text: string; retry?: () => void }) {
  return (
    <div className="error-box">
      <p>{text}</p>
      {retry && (
        <button className="pill" onClick={retry}>
          Try again
        </button>
      )}
    </div>
  );
}

export function LikeButton({ track }: { track: Track }) {
  useLibrary();
  const liked = library.isLiked(track.videoId);
  return (
    <button className={`icon-btn ${liked ? "on" : ""}`} aria-label={liked ? "Remove from Liked Songs" : "Add to Liked Songs"} onClick={() => library.toggleLike(track)}>
      <Icon name={liked ? "heart" : "heartO"} />
    </button>
  );
}

// ---------------------------------------------------------------- helpers

/** Long-press opens the song menu (like holding a song in Apple Music). */
function useLongPress(onLong: () => void) {
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const firedRef = useRef(false);
  const start = useRef<{ x: number; y: number } | null>(null);
  const clear = () => clearTimeout(timer.current);
  return {
    onPointerDown: (e: RPointerEvent) => {
      firedRef.current = false;
      start.current = { x: e.clientX, y: e.clientY };
      clear();
      timer.current = setTimeout(() => {
        firedRef.current = true;
        onLong();
      }, 480);
    },
    onPointerMove: (e: RPointerEvent) => {
      if (start.current && Math.hypot(e.clientX - start.current.x, e.clientY - start.current.y) > 10) clear();
    },
    onPointerUp: clear,
    onPointerCancel: clear,
    onContextMenu: (e: RMouseEvent) => e.preventDefault(),
    fired: () => {
      const f = firedRef.current;
      firedRef.current = false;
      return f;
    },
  };
}

export function totalTime(tracks: Track[]): string {
  const ms = tracks.reduce((a, t) => a + (t.durationMs ?? 0), 0);
  if (!ms) return "";
  const min = Math.round(ms / 60000);
  return min >= 60 ? `${Math.floor(min / 60)} hr ${min % 60} min` : `${min} min`;
}

export { fmtTime };
