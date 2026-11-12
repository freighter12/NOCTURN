import { useState } from "react";
import type { Track } from "../api";
import { fmtBytes, offline, useDownloads } from "../downloads";
import { library, useLibrary } from "../library";
import { useApp } from "../state";
import { Art, Icon, PageHeader, Shelf, Tile, TrackRow, totalTime } from "../ui";

export default function LibraryPage() {
  const app = useApp();
  const lib = useLibrary();
  const dl = useDownloads();
  const empty = !lib.mine.length && !lib.liked.length && !lib.albums.length && !lib.playlists.length && !lib.artists.length;

  return (
    <div className="library">
      <PageHeader
        title="Library"
        right={
          <button className="nav-btn" aria-label="Settings" onClick={() => app.push({ kind: "settings" })}>
            <Icon name="settings" size={20} />
          </button>
        }
      />
      <nav className="lib-links">
        <button onClick={app.openImport}>
          <span className="lib-icon import">
            <Icon name="plus" size={18} />
          </span>
          <span className="lib-label">Import Playlist</span>
          <span className="lib-count">Spotify · YouTube</span>
        </button>
        <button onClick={() => app.push({ kind: "liked" })}>
          <span className="lib-icon liked">
            <Icon name="heart" size={18} />
          </span>
          <span className="lib-label">Liked Songs</span>
          <span className="lib-count">{lib.liked.length || ""}</span>
        </button>
        <button onClick={() => app.push({ kind: "downloads" })}>
          <span className="lib-icon">
            <Icon name="download" size={18} />
          </span>
          <span className="lib-label">Downloads</span>
          <span className="lib-count">{dl.active ? "Downloading…" : dl.tracks.length || ""}</span>
        </button>
        <button onClick={() => app.push({ kind: "stats" })}>
          <span className="lib-icon wrapped">
            <Icon name="chart" size={18} />
          </span>
          <span className="lib-label">Nocturn Wrapped</span>
          <span className="lib-count">Your stats</span>
        </button>
        <button onClick={() => app.push({ kind: "recent" })}>
          <span className="lib-icon">
            <Icon name="note" size={18} />
          </span>
          <span className="lib-label">Recently Played</span>
          <span className="lib-count">{lib.recent.length || ""}</span>
        </button>
        <button onClick={() => app.push({ kind: "saved", what: "playlists" })}>
          <span className="lib-icon">
            <Icon name="queueEnd" size={18} />
          </span>
          <span className="lib-label">Playlists</span>
          <span className="lib-count">{lib.playlists.length || ""}</span>
        </button>
        <button onClick={() => app.push({ kind: "saved", what: "albums" })}>
          <span className="lib-icon">
            <Icon name="album" size={18} />
          </span>
          <span className="lib-label">Albums</span>
          <span className="lib-count">{lib.albums.length || ""}</span>
        </button>
        <button onClick={() => app.push({ kind: "saved", what: "artists" })}>
          <span className="lib-icon">
            <Icon name="mic" size={18} />
          </span>
          <span className="lib-label">Artists</span>
          <span className="lib-count">{lib.artists.length || ""}</span>
        </button>
      </nav>

      {lib.mine.length > 0 && (
        <Shelf title="Your Playlists">
          <div className="grid">
            {lib.mine.map((p) => (
              <Tile key={p.id} card={{ kind: "mine", id: p.id, title: p.title, subtitle: `${p.tracks.length} songs${p.source ? ` · ${p.source}` : ""}`, artwork: p.artwork }} />
            ))}
          </div>
        </Shelf>
      )}

      {empty ? (
        <div className="empty-state">
          <Icon name="heartO" size={40} />
          <h3>Your library is empty</h3>
          <p>Tap the heart on a song, or the + on an album, playlist or artist, and it shows up here. Or import a playlist from Spotify or YouTube with a link.</p>
        </div>
      ) : (
        lib.albums.length + lib.playlists.length > 0 && (
          <Shelf title="Recently Added">
            <div className="grid">
              {[...lib.albums, ...lib.playlists].slice(0, 12).map((c) => (
                <Tile key={c.id} card={c} />
              ))}
            </div>
          </Shelf>
        )
      )}
      <div className="bottom-space" />
    </div>
  );
}

export function MinePage({ id }: { id: string }) {
  const app = useApp();
  useLibrary();
  const p = library.playlist(id);
  const [confirm, setConfirm] = useState(false);
  if (!p) {
    return (
      <div>
        <PageHeader title="Playlist" onBack={app.back} />
        <p className="empty">This playlist was deleted.</p>
      </div>
    );
  }
  const time = totalTime(p.tracks);
  return (
    <div className="collection">
      <PageHeader title={p.title} onBack={app.back} large={false} />
      <header className="col-head">
        <Art src={p.artwork} radius={14} className="col-art" />
        <h1>{p.title}</h1>
        <p className="col-sub">{[`${p.tracks.length} songs`, time, p.source ? `from ${p.source}` : ""].filter(Boolean).join(" · ")}</p>
      </header>
      {p.tracks.length > 0 && <PlayButtons tracks={p.tracks} onPlay={() => app.playList(p.tracks, 0)} onShuffle={() => app.playList(p.tracks, 0, { shuffle: true })} />}
      {p.tracks.map((t, i) => (
        <TrackRow key={t.videoId} track={t} list={p.tracks} index={i} />
      ))}
      {!!p.missing?.length && (
        <details className="missing">
          <summary>{p.missing.length} songs weren't found on YouTube Music</summary>
          <ul>
            {p.missing.map((m, i) => (
              <li key={i}>
                {m.artist} — {m.title}
              </li>
            ))}
          </ul>
        </details>
      )}
      <div className="danger-zone">
        {confirm ? (
          <>
            <span>Delete “{p.title}”?</span>
            <button
              className="pill danger"
              onClick={() => {
                library.removePlaylist(p.id);
                app.back();
                app.toast("Playlist deleted");
              }}
            >
              Delete
            </button>
            <button className="pill" onClick={() => setConfirm(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button className="link danger" onClick={() => setConfirm(true)}>
            Delete Playlist
          </button>
        )}
      </div>
      <div className="bottom-space" />
    </div>
  );
}

export function LikedPage() {
  const app = useApp();
  const lib = useLibrary();
  return (
    <div>
      <PageHeader title="Liked Songs" onBack={app.back} />
      {lib.liked.length > 0 && <PlayButtons tracks={lib.liked} onPlay={() => app.playList(lib.liked, 0)} onShuffle={() => app.playList(lib.liked, 0, { shuffle: true })} />}
      {lib.liked.map((t, i) => (
        <TrackRow key={t.videoId} track={t} list={lib.liked} index={i} />
      ))}
      {!lib.liked.length && <p className="empty">Songs you like show up here.</p>}
      <div className="bottom-space" />
    </div>
  );
}

export function RecentPage() {
  const app = useApp();
  const lib = useLibrary();
  return (
    <div>
      <PageHeader title="Recently Played" onBack={app.back} />
      {lib.recent.map((t, i) => (
        <TrackRow key={t.videoId} track={t} list={lib.recent} index={i} />
      ))}
      {!lib.recent.length && <p className="empty">Songs you play show up here.</p>}
      <div className="bottom-space" />
    </div>
  );
}

export function SavedPage({ what }: { what: "albums" | "playlists" | "artists" }) {
  const app = useApp();
  const lib = useLibrary();
  const title = what === "albums" ? "Albums" : what === "artists" ? "Artists" : "Playlists";
  const list = lib[what];
  return (
    <div>
      <PageHeader title={title} onBack={app.back} />
      <div className="grid">
        {list.map((c) => (
          <Tile key={c.id} card={c} />
        ))}
      </div>
      {!list.length && <p className="empty">Nothing saved yet. Tap + on {what === "artists" ? "an artist" : `a${what === "albums" ? "n album" : " playlist"}`} to keep it here.</p>}
      <div className="bottom-space" />
    </div>
  );
}

export function PlayButtons({ onPlay, onShuffle, tracks }: { onPlay: () => void; onShuffle: () => void; tracks?: Track[] }) {
  return (
    <div className={`play-buttons ${tracks ? "with-dl" : ""}`}>
      <button className="glass-pill" onClick={onPlay}>
        <Icon name="play" size={18} /> Play
      </button>
      <button className="glass-pill" onClick={onShuffle}>
        <Icon name="shuffle" size={18} /> Shuffle
      </button>
      {tracks && <DownloadButton tracks={tracks} />}
    </div>
  );
}

/** Saves a whole album/playlist on the phone. Shows a ring while it works. */
function DownloadButton({ tracks }: { tracks: Track[] }) {
  const app = useApp();
  useDownloads();
  const { saved, waiting } = offline.count(tracks);
  const all = saved === tracks.length;
  const pct = tracks.length ? saved / tracks.length : 0;
  const label = all ? "Downloaded" : waiting ? `Downloading, ${saved} of ${tracks.length}` : "Download";
  return (
    <button
      className={`glass-round dl-btn ${all ? "done" : ""}`}
      aria-label={label}
      onClick={() => {
        if (all) {
          app.toast("All downloaded — plays without internet");
          return;
        }
        if (waiting) {
          app.toast(`Downloading… ${saved} of ${tracks.length} done`);
          return;
        }
        offline
          .add(tracks)
          .then(() => app.toast(`Downloading ${tracks.length - saved} songs`))
          .catch((e) => app.toast(String(e)));
      }}
    >
      {waiting > 0 && (
        <svg className="dl-ring" viewBox="0 0 36 36" aria-hidden="true">
          <circle cx="18" cy="18" r="16" pathLength="100" strokeDasharray={`${Math.max(4, pct * 100)} 100`} />
        </svg>
      )}
      <Icon name={all ? "check" : "download"} size={20} />
    </button>
  );
}

export function DownloadsPage() {
  const app = useApp();
  const dl = useDownloads();
  const [confirm, setConfirm] = useState(false);
  const left = dl.pending.length + (dl.active ? 1 : 0);
  return (
    <div>
      <PageHeader title="Downloads" onBack={app.back} />
      {left > 0 && (
        <div className="dl-status">
          <span className="spinner" />
          <span>
            Downloading {left} {left === 1 ? "song" : "songs"}… {Math.round(dl.progress * 100)}%
          </span>
        </div>
      )}
      {dl.failed.length > 0 && <p className="dl-failed">{dl.failed.length} couldn't be downloaded. Try again from the song's menu.</p>}
      {dl.tracks.length > 0 && <PlayButtons onPlay={() => app.playList(dl.tracks, 0)} onShuffle={() => app.playList(dl.tracks, 0, { shuffle: true })} />}
      {dl.tracks.map((t, i) => (
        <TrackRow key={t.videoId} track={t} list={dl.tracks} index={i} />
      ))}
      {!dl.tracks.length && !left && (
        <div className="empty-state">
          <Icon name="download" size={40} />
          <h3>No downloads yet</h3>
          <p>Tap the download button on a playlist or album, or “Download” in a song's menu. Downloaded songs play with no internet.</p>
        </div>
      )}
      {dl.tracks.length > 0 && (
        <div className="danger-zone">
          {confirm ? (
            <>
              <span>Remove all {dl.tracks.length}?</span>
              <button
                className="pill danger"
                onClick={() => {
                  offline.removeAll();
                  setConfirm(false);
                  app.toast("Downloads removed");
                }}
              >
                Remove
              </button>
              <button className="pill" onClick={() => setConfirm(false)}>
                Cancel
              </button>
            </>
          ) : (
            <button className="link danger" onClick={() => setConfirm(true)}>
              Remove All Downloads ({fmtBytes(dl.bytes)})
            </button>
          )}
        </div>
      )}
      <div className="bottom-space" />
    </div>
  );
}

export { Art };
