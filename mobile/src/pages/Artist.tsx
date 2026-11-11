import { useEffect, useState } from "react";
import { api, errorText, type ArtistPage, type Card } from "../api";
import { library, useLibrary } from "../library";
import { useApp } from "../state";
import { ErrorBox, Icon, Loading, PageHeader, Scroller, Shelf, Tile, TrackRow } from "../ui";

export default function ArtistView({ id, seed }: { id: string; seed?: Card }) {
  const app = useApp();
  useLibrary();
  const [a, setA] = useState<ArtistPage | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setError(null);
    api.artist(id).then(setA).catch((e) => setError(errorText(e)));
  };
  useEffect(load, [id]);

  const name = a?.name ?? seed?.title ?? "";
  const art = a?.artwork ?? seed?.artwork;
  const card: Card = { kind: "artist", id, title: name, subtitle: "Artist", artwork: art };
  const saved = library.isSaved(card);

  return (
    <div className="artist">
      <PageHeader
        title={name}
        onBack={app.back}
        large={false}
        right={
          <button className="nav-btn" aria-label={saved ? "Unfollow" : "Follow"} onClick={() => app.toast(library.toggleSaved(card) ? "Added to Library" : "Removed from Library")}>
            <Icon name={saved ? "check" : "plus"} size={20} />
          </button>
        }
      />
      <header className="artist-hero">
        {art && <img src={art} alt="" />}
        <div className="artist-fade" />
        <div className="artist-name">
          <h1>{name}</h1>
          {a && a.topSongs.length > 0 && (
            <button className="hero-play" aria-label={`Play ${name}`} onClick={() => app.playList(a.topSongs, 0)}>
              <Icon name="play" size={26} />
            </button>
          )}
        </div>
      </header>

      {error && <ErrorBox text={error} retry={load} />}
      {!a && !error && <Loading />}
      {a && a.topSongs.length > 0 && (
        <Shelf title="Top Songs">
          {a.topSongs.map((t, i) => (
            <TrackRow key={t.videoId} track={t} list={a.topSongs} index={i} />
          ))}
          <button className="glass-pill wide" onClick={() => app.startRadio(a.topSongs[0])}>
            <Icon name="radio" size={18} /> {name} Radio
          </button>
        </Shelf>
      )}
      {a?.shelves.map((s, i) =>
        s.cards.length ? (
          <Shelf key={i} title={s.title}>
            <Scroller>
              {s.cards.map((c) => (
                <Tile key={c.kind + c.id} card={c} wide={s.title.toLowerCase().includes("video")} />
              ))}
            </Scroller>
          </Shelf>
        ) : null,
      )}
      <div className="bottom-space" />
    </div>
  );
}
