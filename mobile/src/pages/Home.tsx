import { useEffect, useState } from "react";
import { api, errorText, type Shelf as ShelfT } from "../api";
import { useLibrary } from "../library";
import { useApp } from "../state";
import { Art, ErrorBox, Loading, Scroller, Shelf, Tile, TrackRow } from "../ui";

function greeting(): string {
  const h = new Date().getHours();
  if (h < 5) return "Late night";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

export default function Home() {
  const app = useApp();
  const lib = useLibrary();
  const [shelves, setShelves] = useState<ShelfT[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setError(null);
    api.home().then(setShelves).catch((e) => setError(errorText(e)));
  };
  useEffect(load, []);

  return (
    <div className="home">
      <header className="home-hero">
        <div className="sky" aria-hidden="true">
          {Array.from({ length: 16 }, (_, i) => (
            <i key={i} style={{ left: `${(i * 37) % 100}%`, top: `${(i * 53) % 90}%`, animationDelay: `${(i * 0.37) % 3}s` }} />
          ))}
        </div>
        <svg viewBox="0 0 1024 1024" className="hero-moon" aria-hidden="true">
          <defs>
            <linearGradient id="hm" x1="0.2" y1="0.1" x2="0.8" y2="0.95">
              <stop offset="0" stopColor="#F4F5F8" />
              <stop offset="0.55" stopColor="#C9CCD4" />
              <stop offset="1" stopColor="#7E828E" />
            </linearGradient>
            <mask id="hc">
              <rect width="1024" height="1024" fill="black" />
              <circle cx="512" cy="512" r="300" fill="white" />
              <circle cx="628" cy="430" r="262" fill="black" />
            </mask>
          </defs>
          <rect width="1024" height="1024" fill="url(#hm)" mask="url(#hc)" />
        </svg>
        <p className="eyebrow">Nocturn Music</p>
        <h1 className="hero-title">{greeting()}</h1>
      </header>

      {lib.recent.length > 0 && (
        <Shelf title="Recently Played" onMore={() => app.push({ kind: "recent" })}>
          <Scroller>
            {lib.recent.slice(0, 12).map((t, i) => (
              <button key={t.videoId} className="tile" onClick={() => app.playList(lib.recent, i)}>
                <Art src={t.artwork} radius={10} />
                <span className="tile-title">{t.title}</span>
                <span className="tile-sub">{t.artist}</span>
              </button>
            ))}
          </Scroller>
        </Shelf>
      )}

      {error && <ErrorBox text={error} retry={load} />}
      {!shelves && !error && <Loading label="Finding music" />}
      {shelves?.map((s, si) =>
        s.tracks.length > 0 ? (
          <Shelf key={si} title={s.title}>
            <div className="pick-grid">
              {s.tracks.slice(0, 16).map((t, i) => (
                <div className="pick" key={t.videoId}>
                  <TrackRow track={t} list={s.tracks} index={i} />
                </div>
              ))}
            </div>
          </Shelf>
        ) : (
          <Shelf key={si} title={s.title}>
            <Scroller>
              {s.cards.map((c) => (
                <Tile key={c.kind + c.id} card={c} />
              ))}
            </Scroller>
          </Shelf>
        ),
      )}
      <div className="bottom-space" />
    </div>
  );
}
