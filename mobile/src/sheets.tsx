import { useEffect, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorText, type Track } from "./api";
import { library } from "./library";
import { offline } from "./downloads";
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
        {library.get().mine.slice(0, 3).map((p) => (
          <button
            key={p.id}
            onClick={act(() => {
              library.addToPlaylist(p.id, track);
              app.toast(`Added to ${p.title}`);
            })}
          >
            <Icon name="plus" /> Add to “{p.title}”
          </button>
        ))}
        {offline.has(track.videoId) ? (
          <button onClick={act(() => offline.remove([track.videoId]).then(() => app.toast("Download removed")))}>
            <Icon name="trash" /> Remove Download
          </button>
        ) : (
          <button
            onClick={act(() =>
              offline
                .add([track])
                .then(() => app.toast("Downloading…"))
                .catch((e) => app.toast(String(e))),
            )}
          >
            <Icon name="download" /> Download
          </button>
        )}
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

/** Import a playlist from a Spotify, YouTube Music, YouTube or Deezer link, or a pasted list of songs. */
export function ImportSheet({ onClose }: { onClose: () => void }) {
  const app = useApp();
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number; stage: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  // A long Spotify playlist came in partly: explain how to bring the rest.
  const [partial, setPartial] = useState<{ id: string; count: number } | null>(null);

  useEffect(() => {
    let off: (() => void) | undefined;
    listen<{ done: number; total: number; stage: string }>("import-progress", (e) => setProgress(e.payload)).then((f) => (off = f));
    return () => off?.();
  }, []);

  const paste = async () => {
    try {
      const t = await navigator.clipboard.readText();
      if (t) setLink(t.trim());
    } catch {
      setError("Couldn't read the clipboard — press and hold in the box and choose Paste.");
    }
  };

  const run = async (close: () => void) => {
    if (!link.trim()) return;
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      const r = await api.importPlaylist(link);
      const p = library.addPlaylist({ title: r.title, source: r.source, url: r.url, artwork: r.artwork ?? r.tracks[0]?.artwork, tracks: r.tracks, missing: r.missing });
      player.haptic("success");
      const total = r.tracks.length + r.missing.length;
      app.toast(r.missing.length ? `Imported ${r.tracks.length} of ${total} songs` : `Imported ${r.tracks.length} songs`);
      if (r.truncated) {
        setPartial({ id: p.id, count: total });
        setBusy(false);
        return;
      }
      close();
      if (app.tab !== "library") app.selectTab("library");
      setTimeout(() => app.push({ kind: "mine", id: p.id }), 60);
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  return (
    <Sheet onClose={() => !busy && onClose()}>
      {(close) => (
        <div className="import">
          <h2 className="sheet-title">Import a Playlist</h2>
          {partial ? (
            <>
              <p className="import-help">
                <b>Spotify only lets apps see the first 100 songs</b> of a playlist, so {partial.count} came across.
              </p>
              <p className="import-help">
                To bring every song: open the playlist in Spotify <b>on a computer</b>, click one song, press <b>Ctrl+A</b> then <b>Ctrl+C</b>. Send that to your phone (a note, a message to yourself) and paste it here. Any number of songs works that way.
              </p>
              <button
                className="glass-pill wide"
                onClick={() => {
                  close();
                  if (app.tab !== "library") app.selectTab("library");
                  setTimeout(() => app.push({ kind: "mine", id: partial.id }), 60);
                }}
              >
                Open the playlist
              </button>
            </>
          ) : (
            <>
          <p className="import-help">
            Paste a link to a playlist or album from <b>Spotify</b>, <b>YouTube Music</b>, <b>YouTube</b> or <b>Deezer</b>. Or paste a list of songs, one per line.
          </p>
          <div className="import-field">
            <textarea
              value={link}
              onChange={(e) => setLink(e.target.value)}
              placeholder="https://open.spotify.com/playlist/…"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              rows={link.includes("\n") ? 5 : 1}
              disabled={busy}
            />
            {!link && !busy && (
              <button className="pill" onClick={paste}>
                Paste
              </button>
            )}
          </div>
          {busy && (
            <div className="import-progress">
              <div className="bar">
                <i style={{ width: progress?.total ? `${(progress.done / progress.total) * 100}%` : "8%" }} />
              </div>
              <span>{progress?.total ? `${progress.stage === "reading" ? "Reading the songs" : "Finding songs on YouTube Music"}… ${progress.done} of ${progress.total}` : "Reading the playlist…"}</span>
            </div>
          )}
          {error && <p className="import-error">{error}</p>}
          <button className="glass-pill wide" disabled={busy || !link.trim()} onClick={() => run(close)}>
            {busy ? "Importing…" : "Import"}
          </button>
            </>
          )}
        </div>
      )}
    </Sheet>
  );
}
