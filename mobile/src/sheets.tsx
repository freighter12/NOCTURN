import { useEffect, useState, type ReactNode } from "react";
import type { Track } from "./api";
import { library } from "./library";
import { player } from "./native";
import { useApp } from "./state";
import { Art, Bars, Icon } from "./ui";

function Sheet({ onClose, children, tall }: { onClose: () => void; children: (close: () => void) => ReactNode; tall?: boolean }) {
  const [closing, setClosing] = useState(false);
  const close = () => {
    setClosing(true);
    setTimeout(onClose, 220);
  };
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });
  return (
    <div className={`sheet-wrap ${closing ? "closing" : ""}`} onClick={close}>
      <div className={`sheet ${tall ? "tall" : ""}`} onClick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
        <div className="grabber" onClick={close} />
        {children(close)}
      </div>
    </div>
  );
}


/** The song menu (long-press a song or tap ⋯). */
export function ActionSheet({ track, onClose }: { track: Track; onClose: () => void }) {
  const app = useApp();
  const liked = library.isLiked(track.videoId);
  return (
    <Sheet onClose={onClose}>
      {(close) => {
        const act = (f: () => void) => () => {
          f();
          close();
        };
        return (
          <>
      <div className="sheet-track">
        <Art src={track.artwork} size={56} radius={8} />
        <div>
          <b>{track.title}</b>
          <span>
            {track.artist}
            {track.album ? ` · ${track.album}` : ""}
          </span>
        </div>
      </div>
      <div className="menu">
        <button onClick={act(() => app.playNext([track]))}>
          <Icon name="queueNext" /> Play Next
        </button>
        <button onClick={act(() => app.addToQueue([track]))}>
          <Icon name="queueEnd" /> Add to Queue
        </button>
        <button onClick={act(() => app.toast(library.toggleLike(track) ? "Added to Liked Songs" : "Removed from Liked Songs"))}>
          <Icon name={liked ? "heart" : "heartO"} /> {liked ? "Remove from Liked Songs" : "Add to Liked Songs"}
        </button>
        <button onClick={act(() => app.startRadio(track))}>
          <Icon name="radio" /> Start Radio
        </button>
        {track.albumId && (
          <button onClick={act(() => app.push({ kind: "collection", id: track.albumId! }))}>
            <Icon name="album" /> Go to Album
          </button>
        )}
        {track.artistId && (
          <button onClick={act(() => app.push({ kind: "artist", id: track.artistId! }))}>
            <Icon name="mic" /> Go to Artist
          </button>
        )}
      </div>
          </>
        );
      }}
    </Sheet>
  );
}

/** Up Next. */
export function QueueSheet({ onClose }: { onClose: () => void }) {
  const app = useApp();
  const { queue, player: ps } = app;
  const extra = Math.max(0, ps.queueLength - queue.length);
  return (
    <Sheet onClose={onClose} tall>
      {() => (
        <>
      <h2 className="sheet-title">Up Next</h2>
      <div className="queue">
        {queue.map((t, i) => (
          <button key={t.videoId + i} className={`queue-row ${i === ps.index ? "current" : ""} ${i < ps.index ? "past" : ""}`} onClick={() => player.jump(i)}>
            <Art src={t.artwork} size={44} radius={6} />
            <span className="row-text">
              <span className="row-title">{t.title}</span>
              <span className="row-sub">{t.artist}</span>
            </span>
            {i === ps.index && ps.playing && <Bars />}
          </button>
        ))}
        {extra > 0 && <p className="queue-extra">+ {extra} songs from radio</p>}
        {!queue.length && <p className="empty">Nothing queued.</p>}
      </div>
        </>
      )}
    </Sheet>
  );
}
