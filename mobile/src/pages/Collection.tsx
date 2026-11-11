import { useEffect, useState } from "react";
import { api, errorText, type Card, type Collection } from "../api";
import { library, useLibrary } from "../library";
import { useApp } from "../state";
import { Art, ErrorBox, Icon, Loading, PageHeader, TrackRow, totalTime } from "../ui";
import { PlayButtons } from "./Library";

export default function CollectionPage({ id, seed }: { id: string; seed?: Card }) {
  const app = useApp();
  useLibrary();
  const [c, setC] = useState<Collection | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setError(null);
    api.collection(id).then(setC).catch((e) => setError(errorText(e)));
  };
  useEffect(load, [id]);

  const title = c?.title ?? seed?.title ?? "";
  const art = c?.artwork ?? seed?.artwork;
  const isAlbum = (c?.kind ?? (id.startsWith("MPREb_") ? "album" : "playlist")) === "album";
  const card: Card = { kind: isAlbum ? "album" : "playlist", id, title, subtitle: c?.artist || seed?.subtitle || "", artwork: art };
  const saved = library.isSaved(card);

  return (
    <div className="collection">
      <PageHeader
        title={title}
        onBack={app.back}
        large={false}
        right={
          <button
            className="nav-btn"
            aria-label={saved ? "Remove from Library" : "Add to Library"}
            onClick={() => app.toast(library.toggleSaved(card) ? "Added to Library" : "Removed from Library")}
          >
            <Icon name={saved ? "check" : "plus"} size={20} />
          </button>
        }
      />
      <header className="col-head">
        <Art src={art} radius={14} className="col-art" />
        <h1>{title}</h1>
        {(c?.artist || seed?.subtitle) && (
          <button
            className="col-artist"
            onClick={() => {
              const a = c?.tracks.find((t) => t.artistId)?.artistId;
              if (a && isAlbum) app.push({ kind: "artist", id: a });
            }}
          >
            {c?.artist || seed?.subtitle}
          </button>
        )}
        {c && <p className="col-sub">{[c.subtitle, totalTime(c.tracks) && !c.subtitle.includes("minute") ? totalTime(c.tracks) : ""].filter(Boolean).join(" · ")}</p>}
      </header>

      {c && c.tracks.length > 0 && <PlayButtons onPlay={() => app.playList(c.tracks, 0)} onShuffle={() => app.playList(c.tracks, 0, { shuffle: true })} />}
      {error && <ErrorBox text={error} retry={load} />}
      {!c && !error && <Loading />}
      {c?.tracks.map((t, i) => (
        <TrackRow key={t.videoId + i} track={t} list={c.tracks} index={i} number={isAlbum ? i + 1 : undefined} />
      ))}
      {c?.description && <p className="col-desc">{c.description}</p>}
      <div className="bottom-space" />
    </div>
  );
}
