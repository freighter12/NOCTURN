import { useLibrary } from "../library";
import { useApp } from "../state";
import { Art, Icon, PageHeader, Shelf, Tile, TrackRow } from "../ui";

export default function LibraryPage() {
  const app = useApp();
  const lib = useLibrary();
  const empty = !lib.liked.length && !lib.albums.length && !lib.playlists.length && !lib.artists.length;

  return (
    <div className="library">
      <PageHeader title="Library" />
      <nav className="lib-links">
        <button onClick={() => app.push({ kind: "liked" })}>
          <span className="lib-icon liked">
            <Icon name="heart" size={18} />
          </span>
          <span className="lib-label">Liked Songs</span>
          <span className="lib-count">{lib.liked.length || ""}</span>
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

      {empty ? (
        <div className="empty-state">
          <Icon name="heartO" size={40} />
          <h3>Your library is empty</h3>
          <p>Tap the heart on a song, or the + on an album, playlist or artist, and it shows up here.</p>
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

export function LikedPage() {
  const app = useApp();
  const lib = useLibrary();
  return (
    <div>
      <PageHeader title="Liked Songs" onBack={app.back} />
      {lib.liked.length > 0 && <PlayButtons onPlay={() => app.playList(lib.liked, 0)} onShuffle={() => app.playList(lib.liked, 0, { shuffle: true })} />}
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

export function PlayButtons({ onPlay, onShuffle }: { onPlay: () => void; onShuffle: () => void }) {
  return (
    <div className="play-buttons">
      <button className="glass-pill" onClick={onPlay}>
        <Icon name="play" size={18} /> Play
      </button>
      <button className="glass-pill" onClick={onShuffle}>
        <Icon name="shuffle" size={18} /> Shuffle
      </button>
    </div>
  );
}

export { Art };
