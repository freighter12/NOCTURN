// Nocturn Music: home, search, library (songs, albums, artists, playlists,
// liked, history) and sources. Playback lives in MusicProvider so it carries
// on across the whole app.

import { useEffect, useMemo, useRef, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Note, PageHead, Panel, Seg, Spinner } from "../components/ui";
import { errorMessage, music, type Playlist, type Track } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { relative } from "../lib/format";
import { fmtTime, useMusic } from "../music/MusicProvider";
import { Art } from "../music/Player";
import { ImportedInfo, ImportPanel } from "../music/Import";
import { usePrefs } from "../shell/PrefsProvider";

type Tab = "home" | "search" | "songs" | "albums" | "artists" | "playlists" | "liked" | "history" | "sources";
type View = { kind: "album"; artist: string; album: string } | { kind: "artist"; name: string } | { kind: "playlist"; id: string } | { kind: "import" } | { kind: "refresh"; id: string; url: string } | null;

// ---------------------------------------------------------------- pieces

const songs = (n: number) => `${n} song${n === 1 ? "" : "s"}`;

function TrackRow({ t, n, list, onRemove }: { t: Track; n: number; list: number[]; onRemove?: () => void }) {
  const m = useMusic();
  const [menu, setMenu] = useState(false);
  const playing = m.current?.id === t.id;
  return (
    <div className={`track-row ${playing ? "playing" : ""}`} onDoubleClick={() => m.play(list, n)}>
      <button className="track-num" aria-label={`Play ${t.title}`} onClick={() => m.play(list, n)}>
        <span className="n num">{playing && m.playing ? "♪" : n + 1}</span>
        <I.play />
      </button>
      <Art track={t} size={36} />
      <div className="stack" style={{ gap: 0, minWidth: 0 }}>
        <span className="truncate" style={{ fontWeight: 500 }}>{t.title}</span>
        <span className="small muted truncate">{t.artist}</span>
      </div>
      <span className="small muted truncate">{t.album}</span>
      <button className={`btn ghost icon sm ${t.liked ? "on liked" : ""}`} aria-label={t.liked ? `Unlike ${t.title}` : `Like ${t.title}`} onClick={() => m.setLiked(t.id, !t.liked)}><I.heart /></button>
      <span className="small dim num">{fmtTime(t.duration_ms)}</span>
      <div className="track-menu">
        <button className="btn ghost icon sm" aria-label={`More for ${t.title}`} aria-expanded={menu} onClick={() => setMenu((v) => !v)}>⋯</button>
        {menu && (
          <>
            <div className="ws-backdrop" onClick={() => setMenu(false)} />
            <div className="ws-menu" style={{ left: "auto", right: 0, transform: "none", top: 30 }} role="menu">
              <button role="menuitem" onClick={() => { m.playNext([t.id]); setMenu(false); }}>Play next</button>
              <button role="menuitem" onClick={() => { m.addToQueue([t.id]); setMenu(false); }}>Add to queue</button>
              <div className="ws-sep" />
              {m.playlists.slice(0, 12).map((p) => (
                <button key={p.id} role="menuitem" onClick={async () => { await music.savePlaylist({ ...p, tracks: [...p.tracks, t.id] }); await m.reloadPlaylists(); setMenu(false); }}>Add to {p.name}</button>
              ))}
              <AddToNew t={t} done={() => setMenu(false)} />
              {onRemove && (
                <>
                  <div className="ws-sep" />
                  <button role="menuitem" onClick={() => { onRemove(); setMenu(false); }}>Remove from this playlist</button>
                </>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function AddToNew({ t, done }: { t: Track; done: () => void }) {
  const m = useMusic();
  return (
    <button role="menuitem" onClick={async () => { await music.savePlaylist({ name: "New playlist", description: "", cover: { track_id: t.id }, tracks: [t.id] }); await m.reloadPlaylists(); done(); }}>
      <I.plus /> New playlist with this
    </button>
  );
}

function TrackList({ tracks, onRemove, header = true }: { tracks: Track[]; onRemove?: (i: number) => void; header?: boolean }) {
  const ids = useMemo(() => tracks.map((t) => t.id), [tracks]);
  if (!tracks.length) return <div className="small dim">No songs here yet.</div>;
  return (
    <div className="track-list">
      {header && (
        <div className="track-row head small dim">
          <span>#</span><span /><span>Title</span><span>Album</span><span /><span>Time</span><span />
        </div>
      )}
      {tracks.map((t, i) => <TrackRow key={`${t.id}-${i}`} t={t} n={i} list={ids} onRemove={onRemove ? () => onRemove(i) : undefined} />)}
    </div>
  );
}

function Shelf({ title, children, action }: { title: string; children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <section className="shelf">
      <div className="row between"><div className="section-title" style={{ margin: "0 0 10px" }}>{title}</div>{action}</div>
      <div className="shelf-row">{children}</div>
    </section>
  );
}

function Card({ art, title, sub, onOpen, onPlay }: { art: React.ReactNode; title: string; sub: string; onOpen: () => void; onPlay?: () => void }) {
  return (
    <div className="music-card">
      <button className="mc-open" onClick={onOpen}>
        {art}
        <span className="truncate" style={{ fontWeight: 600, marginTop: 8 }}>{title}</span>
        <span className="small muted truncate">{sub}</span>
      </button>
      {onPlay && <button className="btn primary icon mc-play" aria-label={`Play ${title}`} onClick={onPlay}><I.play /></button>}
    </div>
  );
}

function PlaylistArt({ p, size, byId }: { p: Playlist; size: number; byId: Map<number, Track> }) {
  const t = p.cover.track_id ? byId.get(p.cover.track_id) : byId.get(p.tracks[0]);
  if (p.cover.glyph || p.cover.colour || !t?.has_art) {
    return <span className="art art-empty" style={{ width: size, height: size, background: p.cover.colour ?? undefined, fontSize: size * 0.34 }}>{p.cover.glyph || <I.music />}</span>;
  }
  return <Art track={t} size={size} />;
}

// --------------------------------------------------------------- detail views

function PlaylistView({ id, onBack, onRefresh }: { id: string; onBack: () => void; onRefresh: (url: string) => void }) {
  const m = useMusic();
  const { toast } = useApp();
  const p = m.playlists.find((x) => x.id === id);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(p?.name ?? "");
  const [desc, setDesc] = useState(p?.description ?? "");
  const [glyph, setGlyph] = useState(p?.cover.glyph ?? "");
  const [colour, setColour] = useState(p?.cover.colour ?? "#3b4a6b");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const drag = useRef<number | null>(null);
  useEffect(() => {
    setName(p?.name ?? "");
    setDesc(p?.description ?? "");
  }, [p?.name, p?.description]);
  if (!p) return <Empty title="Playlist not found" />;
  const tracks = p.tracks.map((x) => m.byId.get(x)).filter((t): t is Track => !!t);
  const total = tracks.reduce((a, t) => a + t.duration_ms, 0);
  const save = async (patch: Partial<Playlist>) => {
    try {
      await music.savePlaylist({ ...p, ...patch });
      await m.reloadPlaylists();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save", detail: errorMessage(e) });
    }
  };
  return (
    <div className="stack" style={{ gap: 16 }}>
      <button className="btn ghost sm" style={{ alignSelf: "flex-start", justifySelf: "start" }} onClick={onBack}>← Back</button>
      <div className="detail-head">
        <PlaylistArt p={p} size={168} byId={m.byId} />
        <div className="stack" style={{ gap: 6, flex: 1, minWidth: 0 }}>
          <div className="eyebrow small dim">Playlist</div>
          {editing ? (
            <div className="stack" style={{ gap: 8 }}>
              <input className="input" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} aria-label="Playlist name" />
              <textarea className="textarea" rows={2} maxLength={300} value={desc} onChange={(e) => setDesc(e.target.value)} aria-label="Description" />
              <div className="row" style={{ gap: 8 }}>
                <input className="input" style={{ width: 80 }} maxLength={2} placeholder="🎧" value={glyph} onChange={(e) => setGlyph(e.target.value)} aria-label="Cover text" />
                <input type="color" value={colour} onChange={(e) => setColour(e.target.value)} aria-label="Cover colour" />
                <button className="btn ghost sm" onClick={() => { setGlyph(""); save({ cover: { track_id: p.tracks[0] ?? null } }); setEditing(false); }}>Use album art</button>
                <button className="btn primary sm" disabled={!name.trim()} onClick={() => { save({ name, description: desc, cover: { glyph: glyph || null, colour: glyph ? colour : null, track_id: glyph ? null : p.cover.track_id } }); setEditing(false); }}>Save</button>
                <button className="btn ghost sm" onClick={() => setEditing(false)}>Cancel</button>
              </div>
            </div>
          ) : (
            <>
              <h1 style={{ margin: 0 }}>{p.name}</h1>
              {p.description && <div className="muted">{p.description}</div>}
              <div className="small dim">{tracks.length} songs · {fmtTime(total)}</div>
            </>
          )}
          {!editing && (
            <div className="row" style={{ gap: 6, marginTop: 6 }}>
              <button className="btn primary" disabled={!tracks.length} onClick={() => m.play(tracks.map((t) => t.id))}><I.play /> Play</button>
              <button className="btn" disabled={!tracks.length} onClick={() => { m.setShuffle(true); m.play(tracks.map((t) => t.id)); }}><I.shuffle /> Shuffle</button>
              <button className="btn ghost" onClick={() => setEditing(true)}>Edit</button>
              <button className="btn ghost" onClick={async () => { await music.savePlaylist({ name: `${p.name} (copy)`, description: p.description, cover: p.cover, tracks: p.tracks }); await m.reloadPlaylists(); toast({ tone: "good", title: "Playlist duplicated" }); }}>Duplicate</button>
              {confirmDelete ? (
                <button className="btn" onClick={async () => { await music.deletePlaylist(p.id); await m.reloadPlaylists(); onBack(); }}>Delete playlist</button>
              ) : (
                <button className="btn ghost" onClick={() => setConfirmDelete(true)}>Delete…</button>
              )}
            </div>
          )}
        </div>
      </div>
      <ImportedInfo playlist={p} onRefresh={onRefresh} />
      <div className="small dim">Drag songs to reorder.</div>
      <div className="track-list">
        {tracks.map((t, i) => (
          <div
            key={`${t.id}-${i}`}
            draggable
            onDragStart={() => (drag.current = i)}
            onDragOver={(e) => e.preventDefault()}
            onDrop={() => {
              const from = drag.current;
              drag.current = null;
              if (from == null || from === i) return;
              const next = [...p.tracks];
              const [x] = next.splice(from, 1);
              next.splice(i, 0, x);
              save({ tracks: next });
            }}
          >
            <TrackRow t={t} n={i} list={tracks.map((x) => x.id)} onRemove={() => save({ tracks: p.tracks.filter((_, k) => k !== i) })} />
          </div>
        ))}
        {!tracks.length && <div className="small dim">Add songs from the ⋯ menu on any song.</div>}
      </div>
    </div>
  );
}

function AlbumView({ artist, album, onBack }: { artist: string; album: string; onBack: () => void }) {
  const m = useMusic();
  const tracks = m.tracks.filter((t) => t.album_artist === artist && t.album === album);
  const first = tracks[0];
  return (
    <div className="stack" style={{ gap: 16 }}>
      <button className="btn ghost sm" style={{ alignSelf: "flex-start", justifySelf: "start" }} onClick={onBack}>← Back</button>
      <div className="detail-head">
        <Art track={first} size={168} />
        <div className="stack" style={{ gap: 6 }}>
          <div className="eyebrow small dim">Album</div>
          <h1 style={{ margin: 0 }}>{album}</h1>
          <div className="muted">{artist}{first?.year ? ` · ${first.year}` : ""} · {tracks.length} songs</div>
          <div className="row" style={{ gap: 6, marginTop: 6 }}>
            <button className="btn primary" onClick={() => m.play(tracks.map((t) => t.id))}><I.play /> Play</button>
            <button className="btn" onClick={() => m.addToQueue(tracks.map((t) => t.id))}>Add to queue</button>
          </div>
        </div>
      </div>
      <TrackList tracks={tracks} />
    </div>
  );
}

function ArtistView({ name, onBack, openAlbum }: { name: string; onBack: () => void; openAlbum: (a: string, al: string) => void }) {
  const m = useMusic();
  const tracks = m.tracks.filter((t) => t.artist === name || t.album_artist === name);
  const albums = [...new Map(tracks.map((t) => [`${t.album_artist}|${t.album}`, t])).values()];
  const top = [...tracks].sort((a, b) => b.plays - a.plays).slice(0, 10);
  return (
    <div className="stack" style={{ gap: 16 }}>
      <button className="btn ghost sm" style={{ alignSelf: "flex-start", justifySelf: "start" }} onClick={onBack}>← Back</button>
      <div>
        <div className="eyebrow small dim">Artist</div>
        <h1 style={{ margin: "4px 0" }}>{name}</h1>
        <div className="row" style={{ gap: 6 }}>
          <button className="btn primary" onClick={() => m.play(tracks.map((t) => t.id))}><I.play /> Play all</button>
          <button className="btn" onClick={() => { m.setShuffle(true); m.play(tracks.map((t) => t.id)); }}><I.shuffle /> Shuffle</button>
        </div>
      </div>
      <Shelf title="Albums">
        {albums.map((t) => <Card key={`${t.album_artist}|${t.album}`} art={<Art track={t} size={150} />} title={t.album} sub={t.year ? String(t.year) : t.album_artist} onOpen={() => openAlbum(t.album_artist, t.album)} />)}
      </Shelf>
      <div className="section-title" style={{ margin: 0 }}>Most played</div>
      <TrackList tracks={top} header={false} />
    </div>
  );
}

// -------------------------------------------------------------------- tabs

function Sources() {
  const m = useMusic();
  const { toast } = useApp();
  const folders = useLoad(() => music.folders(), []);
  const [path, setPath] = useState("");
  const [scanning, setScanning] = useState<string | null>(null);
  async function scan() {
    setScanning("Starting…");
    try {
      const r = await music.scan((p) => setScanning(`${p.folder} — ${p.files} files`));
      await m.reload();
      toast({ tone: "good", title: "Library updated", detail: `${r.files} files in ${r.folders} folder(s): ${r.added} added, ${r.updated} updated, ${r.removed} removed${r.unreadable ? `, ${r.unreadable} couldn't be read` : ""}.` });
    } catch (e) {
      toast({ tone: "critical", title: "Scan failed", detail: errorMessage(e) });
    } finally {
      setScanning(null);
    }
  }
  async function add() {
    try {
      await music.addFolder(path);
      setPath("");
      folders.reload();
      await scan();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't add that folder", detail: errorMessage(e) });
    }
  }
  return (
    <div className="stack" style={{ gap: 14 }}>
      <Panel title="Your music folders" sub="Nocturn reads music files where they are. Nothing is copied, changed or uploaded." actions={<button className="btn sm" disabled={!!scanning || !folders.data?.length} onClick={scan}>{scanning ? <Spinner /> : <I.refresh />} Rescan</button>}>
        {folders.error && <ErrorNote message={folders.error} />}
        <div className="list">
          {(folders.data ?? []).map((f) => (
            <div key={f} className="list-item" style={{ alignItems: "center" }}>
              <div className="body"><div className="title mono selectable">{f}</div></div>
              <button className="btn ghost sm" onClick={async () => { await music.removeFolder(f); folders.reload(); m.reload(); }}>Remove</button>
            </div>
          ))}
          {folders.data && folders.data.length === 0 && <div className="small dim">No folders yet.</div>}
        </div>
        <div className="row mt-2" style={{ gap: 8 }}>
          <input className="input" style={{ flex: 1 }} placeholder={String.raw`e.g. C:\Users\you\Music`} value={path} onChange={(e) => setPath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && path.trim() && add()} aria-label="Folder to add" />
          <button className="btn primary" disabled={!path.trim() || !!scanning} onClick={add}>Add folder</button>
        </div>
        <div className="hint small dim mt-1">In File Explorer, Shift + right-click a folder and choose "Copy as path", then paste it here. MP3, FLAC, M4A/AAC, OGG, Opus, WAV and WMA are supported.</div>
        {scanning && <div className="small mt-1"><Spinner /> {scanning}</div>}
      </Panel>
      <Note>
        Nocturn Music has no ads and never will. Today it plays music you own. A Nocturn catalogue of licensed and independent music is planned as another source; Nocturn won't download, rip or capture music from other services.
      </Note>
    </div>
  );
}

function Onboarding({ go }: { go: () => void }) {
  return (
    <Panel>
      <Empty title="Your music, no ads">
        Add a folder of music you own and Nocturn builds your library — albums, artists, playlists and more.
        <div className="mt-2"><button className="btn primary" onClick={go}>Add a music folder</button></div>
      </Empty>
    </Panel>
  );
}

// -------------------------------------------------------------------- page

export function Music() {
  const m = useMusic();
  const { update } = usePrefs();
  const [tab, setTab] = useState<Tab>("home");
  const [view, setView] = useState<View>(null);
  const [q, setQ] = useState("");
  const history = useLoad(() => music.history(100), [m.tracks.length, m.current?.id]);
  const rec = useLoad(() => music.recommended(), [m.tracks.length]);
  const { toast } = useApp();

  const albums = useMemo(() => [...new Map(m.tracks.map((t) => [`${t.album_artist}|${t.album}`, t])).values()].sort((a, b) => a.album.localeCompare(b.album)), [m.tracks]);
  const artists = useMemo(() => {
    const c = new Map<string, { name: string; songs: number; plays: number; sample: Track }>();
    for (const t of m.tracks) {
      const a = c.get(t.artist) ?? { name: t.artist, songs: 0, plays: 0, sample: t };
      a.songs++;
      a.plays += t.plays;
      c.set(t.artist, a);
    }
    return [...c.values()];
  }, [m.tracks]);

  const open = (v: View) => setView(v);
  const ql = q.trim().toLowerCase();
  const results = ql
    ? {
        songs: m.tracks.filter((t) => `${t.title} ${t.artist} ${t.album}`.toLowerCase().includes(ql)).slice(0, 40),
        artists: artists.filter((a) => a.name.toLowerCase().includes(ql)).slice(0, 12),
        albums: albums.filter((t) => `${t.album} ${t.album_artist}`.toLowerCase().includes(ql)).slice(0, 12),
        playlists: m.playlists.filter((p) => p.name.toLowerCase().includes(ql)),
      }
    : null;

  const recentIds = [...new Set((history.data ?? []).map((h) => h.track_id))].filter((id) => m.byId.has(id)).slice(0, 12);
  const recentlyAdded = [...m.tracks].sort((a, b) => b.added_at.localeCompare(a.added_at)).slice(0, 12);
  const topArtists = [...artists].filter((a) => a.plays > 0).sort((a, b) => b.plays - a.plays).slice(0, 10);
  const liked = m.tracks.filter((t) => t.liked);

  async function newPlaylist() {
    try {
      const p = await music.savePlaylist({ name: `Playlist ${m.playlists.length + 1}`, description: "", cover: {}, tracks: [] });
      await m.reloadPlaylists();
      setView({ kind: "playlist", id: p.id });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't create a playlist", detail: errorMessage(e) });
    }
  }

  const body = () => {
    if (view?.kind === "playlist") { const id = view.id; return <PlaylistView id={id} onBack={() => setView(null)} onRefresh={(url) => setView({ kind: "refresh", id, url })} />; }
    if (view?.kind === "import") return <ImportPanel onDone={(p) => setView({ kind: "playlist", id: p.id })} onCancel={() => setView(null)} />;
    if (view?.kind === "refresh") {
      const pl = m.playlists.find((x) => x.id === view.id);
      if (pl) return <ImportPanel refresh={{ playlist: pl, url: view.url }} onDone={(p) => setView({ kind: "playlist", id: p.id })} onCancel={() => setView({ kind: "playlist", id: pl.id })} />;
    }
    if (view?.kind === "album") return <AlbumView artist={view.artist} album={view.album} onBack={() => setView(null)} />;
    if (view?.kind === "artist") return <ArtistView name={view.name} onBack={() => setView(null)} openAlbum={(artist, album) => setView({ kind: "album", artist, album })} />;
    if (m.loaded && m.tracks.length === 0 && tab !== "sources") return <Onboarding go={() => setTab("sources")} />;
    switch (tab) {
      case "home":
        return (
          <div className="stack" style={{ gap: 22 }}>
            {m.current && (
              <Shelf title="Continue listening">
                <Card art={<Art track={m.current} size={150} />} title={m.current.title} sub={m.current.artist} onOpen={() => update((p) => ({ ...p, music: { ...p.music, playerMode: "full" } }))} onPlay={m.toggle} />
              </Shelf>
            )}
            {m.playlists.length > 0 && (
              <Shelf title="Your playlists" action={<span className="row" style={{ gap: 4 }}><button className="btn ghost sm" onClick={() => open({ kind: "import" })}>Import</button><button className="btn ghost sm" onClick={newPlaylist}><I.plus /> New</button></span>}>
                {m.playlists.map((p) => <Card key={p.id} art={<PlaylistArt p={p} size={150} byId={m.byId} />} title={p.name} sub={songs(p.tracks.length)} onOpen={() => open({ kind: "playlist", id: p.id })} onPlay={() => m.play(p.tracks)} />)}
              </Shelf>
            )}
            {(rec.data?.length ?? 0) > 0 && (
              <Shelf title="Recommended for you" action={<span className="small dim">From your own listening, on this PC</span>}>
                {rec.data!.map((id) => m.byId.get(id)).filter((t): t is Track => !!t).slice(0, 12).map((t) => <Card key={t.id} art={<Art track={t} size={150} />} title={t.title} sub={t.artist} onOpen={() => open({ kind: "album", artist: t.album_artist, album: t.album })} onPlay={() => m.play(rec.data!, rec.data!.indexOf(t.id))} />)}
              </Shelf>
            )}
            {recentIds.length > 0 && (
              <Shelf title="Recently played">
                {recentIds.map((id) => m.byId.get(id)!).map((t, i) => <Card key={t.id} art={<Art track={t} size={150} />} title={t.title} sub={t.artist} onOpen={() => open({ kind: "album", artist: t.album_artist, album: t.album })} onPlay={() => m.play(recentIds, i)} />)}
              </Shelf>
            )}
            {topArtists.length > 0 && (
              <Shelf title="Your favourite artists">
                {topArtists.map((a) => <Card key={a.name} art={<Art track={a.sample} size={150} className="round" />} title={a.name} sub={songs(a.songs)} onOpen={() => open({ kind: "artist", name: a.name })} />)}
              </Shelf>
            )}
            <Shelf title="Recently added">
              {recentlyAdded.map((t) => <Card key={t.id} art={<Art track={t} size={150} />} title={t.title} sub={t.artist} onOpen={() => open({ kind: "album", artist: t.album_artist, album: t.album })} onPlay={() => m.play(recentlyAdded.map((x) => x.id), recentlyAdded.indexOf(t))} />)}
            </Shelf>
          </div>
        );
      case "search":
        return (
          <div className="stack" style={{ gap: 18 }}>
            <input className="input" autoFocus style={{ fontSize: 15, height: 40 }} placeholder="Songs, artists, albums or playlists" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search music" />
            {!results && <div className="small dim">Start typing to search your library.</div>}
            {results && results.songs.length + results.artists.length + results.albums.length + results.playlists.length === 0 && <Empty title={`Nothing found for "${q}"`} />}
            {results && results.artists.length > 0 && <Shelf title="Artists">{results.artists.map((a) => <Card key={a.name} art={<Art track={a.sample} size={120} className="round" />} title={a.name} sub={songs(a.songs)} onOpen={() => open({ kind: "artist", name: a.name })} />)}</Shelf>}
            {results && results.albums.length > 0 && <Shelf title="Albums">{results.albums.map((t) => <Card key={`${t.album_artist}|${t.album}`} art={<Art track={t} size={120} />} title={t.album} sub={t.album_artist} onOpen={() => open({ kind: "album", artist: t.album_artist, album: t.album })} />)}</Shelf>}
            {results && results.playlists.length > 0 && <Shelf title="Playlists">{results.playlists.map((p) => <Card key={p.id} art={<PlaylistArt p={p} size={120} byId={m.byId} />} title={p.name} sub={songs(p.tracks.length)} onOpen={() => open({ kind: "playlist", id: p.id })} />)}</Shelf>}
            {results && results.songs.length > 0 && <><div className="section-title" style={{ margin: 0 }}>Songs</div><TrackList tracks={results.songs} /></>}
          </div>
        );
      case "songs":
        return <TrackList tracks={m.tracks} />;
      case "albums":
        return <div className="music-grid">{albums.map((t) => <Card key={`${t.album_artist}|${t.album}`} art={<Art track={t} size={150} />} title={t.album} sub={t.album_artist} onOpen={() => open({ kind: "album", artist: t.album_artist, album: t.album })} onPlay={() => m.play(m.tracks.filter((x) => x.album === t.album && x.album_artist === t.album_artist).map((x) => x.id))} />)}</div>;
      case "artists":
        return <div className="music-grid">{[...artists].sort((a, b) => a.name.localeCompare(b.name)).map((a) => <Card key={a.name} art={<Art track={a.sample} size={150} className="round" />} title={a.name} sub={songs(a.songs)} onOpen={() => open({ kind: "artist", name: a.name })} />)}</div>;
      case "playlists":
        return (
          <div className="stack" style={{ gap: 12 }}>
            <div className="row" style={{ gap: 8 }}>
              <button className="btn primary" onClick={newPlaylist}><I.plus /> New playlist</button>
              <button className="btn" onClick={() => open({ kind: "import" })}><I.queue /> Import playlist</button>
              <span className="small dim">Paste a Spotify, YouTube or Deezer link, or a list of songs.</span>
            </div>
            {m.playlists.length === 0 ? <div className="small dim">No playlists yet. Create one, or use "New playlist with this" on any song.</div> : (
              <div className="music-grid">{m.playlists.map((p) => <Card key={p.id} art={<PlaylistArt p={p} size={150} byId={m.byId} />} title={p.name} sub={songs(p.tracks.length)} onOpen={() => open({ kind: "playlist", id: p.id })} onPlay={() => m.play(p.tracks)} />)}</div>
            )}
          </div>
        );
      case "liked":
        return (
          <div className="stack" style={{ gap: 12 }}>
            <div className="row" style={{ gap: 8 }}>
              <button className="btn primary" disabled={!liked.length} onClick={() => m.play(liked.map((t) => t.id))}><I.play /> Play liked songs</button>
              <span className="small dim">{liked.length} songs</span>
            </div>
            <TrackList tracks={liked} />
          </div>
        );
      case "history":
        return (
          <div className="stack" style={{ gap: 12 }}>
            <div className="row between">
              <span className="small dim">What you listened to, kept on this PC for two years.</span>
              <button className="btn ghost sm" disabled={!history.data?.length} onClick={async () => { await music.clearHistory(); history.reload(); m.reload(); }}>Clear history</button>
            </div>
            <div className="list">
              {(history.data ?? []).map((h, i) => {
                const t = m.byId.get(h.track_id);
                if (!t) return null;
                return (
                  <div key={i} className="list-item" style={{ alignItems: "center" }}>
                    <Art track={t} size={36} />
                    <div className="body"><div className="title">{t.title}</div><div className="desc">{t.artist} · {relative(h.played_at)}</div></div>
                    <button className="btn ghost sm icon" aria-label={`Play ${t.title}`} onClick={() => m.play([t.id])}><I.play /></button>
                  </div>
                );
              })}
              {history.data?.length === 0 && <div className="small dim">Nothing played yet.</div>}
            </div>
          </div>
        );
      case "sources":
        return <Sources />;
    }
  };

  return (
    <div className="page">
      <PageHead
        eyebrow="Nocturn Music"
        title="Music"
        desc={m.tracks.length ? `${m.tracks.length} songs · ${albums.length} albums · ${artists.length} artists — no ads, ever.` : "Your music library, playlists and queue — no ads, ever."}
        actions={m.current && <button className="btn" onClick={() => update((p) => ({ ...p, music: { ...p.music, playerMode: "full" } }))}><I.music /> Now playing</button>}
      />
      <Seg<Tab>
        value={tab}
        onChange={(t) => { setTab(t); setView(null); }}
        options={[
          { value: "home", label: "Home" },
          { value: "search", label: "Search" },
          { value: "songs", label: "Songs" },
          { value: "albums", label: "Albums" },
          { value: "artists", label: "Artists" },
          { value: "playlists", label: "Playlists" },
          { value: "liked", label: "Liked" },
          { value: "history", label: "History" },
          { value: "sources", label: "Sources" },
        ]}
      />
      <div className="mt-2">{body()}</div>
    </div>
  );
}

