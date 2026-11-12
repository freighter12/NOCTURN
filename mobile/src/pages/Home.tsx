import { useEffect, useState } from "react";
import { api, errorText, type Shelf as ShelfT } from "../api";
import { useLibrary } from "../library";
import { useApp } from "../state";
import { eyebrowText, greetingText, homePrefs, type HomePrefs } from "../greeting";
import { Art, ErrorBox, Loading, Scroller, Shelf, Tile, TrackRow } from "../ui";

/** The greeting at the top of Home. Also used as the live preview in Settings › Home Screen. */
export function HomeHero({ prefs, onEdit, preview }: { prefs: HomePrefs; onEdit?: () => void; preview?: boolean }) {
  const [, tick] = useState(0);
  // refresh the greeting when the part of the day changes
  useEffect(() => {
    const id = setInterval(() => tick((n) => n + 1), 60_000);
    return () => clearInterval(id);
  }, []);
  return (
    <header className={`home-hero ${prefs.big ? "big" : ""} ${preview ? "preview" : ""} ${prefs.moon ? "with-moon" : ""}`} onClick={onEdit} aria-label={onEdit ? "Edit greeting" : undefined}>
      {prefs.stars && (
        <div className="sky" aria-hidden="true">
          {Array.from({ length: 16 }, (_, i) => (
            <i key={i} style={{ left: `${(i * 37) % 100}%`, top: `${(i * 53) % 90}%`, animationDelay: `${(i * 0.37) % 3}s` }} />
          ))}
        </div>
      )}
      {prefs.moon && (
      <svg viewBox="0 0 1024 1024" className="hero-moon" aria-hidden="true">
        <defs>
          <linearGradient id={preview ? "hm-p" : "hm"} x1="0.2" y1="0.1" x2="0.8" y2="0.95">
            <stop offset="0" stopColor="#F4F5F8" />
            <stop offset="0.55" stopColor="#C9CCD4" />
            <stop offset="1" stopColor="#7E828E" />
          </linearGradient>
          <mask id={preview ? "hc-p" : "hc"}>
            <rect width="1024" height="1024" fill="black" />
            <circle cx="512" cy="512" r="300" fill="white" />
            <circle cx="628" cy="430" r="262" fill="black" />
          </mask>
        </defs>
        <rect width="1024" height="1024" fill={preview ? "url(#hm-p)" : "url(#hm)"} mask={preview ? "url(#hc-p)" : "url(#hc)"} />
      </svg>
      )}
      <p className="eyebrow">{eyebrowText(prefs)}</p>
      <h1 className="hero-title">{greetingText(prefs)}</h1>
    </header>
  );
}


export default function Home() {
  const app = useApp();
  const lib = useLibrary();
  const prefs = homePrefs(lib);
  const [shelves, setShelves] = useState<ShelfT[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setError(null);
    api.home().then(setShelves).catch((e) => setError(errorText(e)));
  };
  useEffect(load, []);

  return (
    <div className="home">
      <HomeHero prefs={prefs} onEdit={() => app.push({ kind: "settings", section: "home" })} />

      {prefs.recent && lib.recent.length > 0 && (
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

      {prefs.picks && error && <ErrorBox text={error} retry={load} />}
      {prefs.picks && !shelves && !error && <Loading label="Finding music" />}
      {prefs.picks && shelves?.map((s, si) =>
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
