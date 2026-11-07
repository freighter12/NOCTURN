// Playlist import: paste a Spotify / YouTube / Deezer link, a list of songs,
// a CSV export or an M3U file, see what's in the library, and save it as a
// playlist. Only song names are read from the service — nothing is downloaded.
// Imported playlists remember the songs that were missing, so adding files
// later fills the gaps.

import { useEffect, useMemo, useRef, useState } from "react";
import { I } from "../components/Icons";
import { ErrorNote, Note, Panel, Seg, Spinner } from "../components/ui";
import { errorMessage, music, type ImportInfo, type ImportItem, type ImportPreview, type Playlist } from "../lib/api";
import { useApp } from "../lib/app";
import { fmtTime, useMusic } from "./MusicProvider";
import { streamSongs, useStreamStatus } from "./Streaming";

type Mode = "link" | "list";
type Filter = "all" | "found" | "missing";

const line = (i: Pick<ImportItem, "title" | "artist">) => (i.artist ? `${i.artist} - ${i.title}` : i.title);

async function copyMissing(items: ImportItem[], toast: ReturnType<typeof useApp>["toast"]) {
  try {
    await navigator.clipboard.writeText(items.map(line).join("\n"));
    toast({ tone: "good", title: `Copied ${items.length} song${items.length === 1 ? "" : "s"}` });
  } catch {
    toast({ tone: "warning", title: "Couldn't copy", detail: "Windows didn't allow access to the clipboard." });
  }
}

/** Import a playlist. With `refresh`, re-reads that playlist's original link and updates it in place. */
export function ImportPanel({ refresh, onDone, onCancel }: { refresh?: { playlist: Playlist; url: string }; onDone: (p: Playlist) => void; onCancel: () => void }) {
  const m = useMusic();
  const { toast } = useApp();
  const [mode, setMode] = useState<Mode>("link");
  const [link, setLink] = useState(refresh?.url ?? "");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [name, setName] = useState("");
  const [skip, setSkip] = useState<Set<number>>(new Set());
  const [filter, setFilter] = useState<Filter>("all");
  const file = useRef<HTMLInputElement>(null);
  const streaming = useStreamStatus();
  const [stream, setStream] = useState(true);
  const [streamProgress, setStreamProgress] = useState<string | null>(null);

  async function find(input: { link?: string; text?: string }) {
    setBusy(true);
    setError(null);
    setPreview(null);
    try {
      const p = await music.importPreview(input);
      setPreview(p);
      setName(refresh?.playlist.name ?? p.name);
      setSkip(new Set());
      setFilter("all");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  // Updating from the original link starts straight away.
  useEffect(() => {
    if (refresh) find({ link: refresh.url });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function save() {
    if (!preview) return;
    setBusy(true);
    try {
      let items = preview.items.map((it, i) => (skip.has(i) ? { ...it, track_id: null, confidence: null } : it));
      let streamed = 0;
      if (streaming?.enabled && stream) {
        // Find the missing songs on YouTube and add them as streaming songs.
        const want = items.map((it, i) => ({ it, i })).filter(({ it, i }) => it.track_id == null && !skip.has(i));
        if (want.length) {
          setStreamProgress(`Finding songs on YouTube… 0 of ${want.length}`);
          const res = await streamSongs(want.map(({ it }) => it), (p) => setStreamProgress(`Finding songs on YouTube… ${p.done} of ${p.total} (${p.found} found)`));
          items = items.map((it) => ({ ...it }));
          res.forEach((r) => {
            const at = want[r.index]?.i;
            if (at != null && r.track_id != null) {
              items[at] = { ...items[at], track_id: r.track_id, confidence: "stream" };
              streamed++;
            }
          });
          await m.reload();
        }
      }
      const p = await music.importSave({ playlist_id: refresh?.playlist.id ?? null, name: name.trim() || preview.name, source: preview.source, url: preview.url, items });
      await m.reloadPlaylists();
      const n = items.filter((x) => x.track_id != null).length;
      const local = n - streamed;
      toast({ tone: "good", title: refresh ? `Updated ${p.name}` : `Created ${p.name}`, detail: streamed ? `${local} from your library, ${streamed} streamed from YouTube${n < items.length ? `, ${items.length - n} not found` : ""}.` : `${n} of ${items.length} songs are in your library.` });
      onDone(p);
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save the playlist", detail: errorMessage(e) });
    } finally {
      setBusy(false);
      setStreamProgress(null);
    }
  }

  const readFile = (f: File | undefined) => {
    if (!f) return;
    if (f.size > 2 * 1024 * 1024) return setError("That file is too big to be a playlist (2 MB max).");
    f.text().then((t) => {
      setText(t);
      setMode("list");
    });
  };

  const shown = useMemo(
    () => (preview?.items ?? []).map((it, i) => ({ it, i })).filter(({ it }) => (filter === "all" ? true : filter === "found" ? it.track_id != null : it.track_id == null)),
    [preview, filter],
  );
  const chosen = preview ? preview.items.filter((it, i) => it.track_id != null && !skip.has(i)).length : 0;
  const missing = preview ? preview.items.filter((it) => it.track_id == null) : [];

  return (
    <Panel
      title={refresh ? `Update ${refresh.playlist.name} from its link` : "Import a playlist"}
      sub="Reads the song list only and finds those songs in your library. Nothing is downloaded."
      actions={<button className="btn ghost sm" onClick={onCancel}>Cancel</button>}
    >
      {!refresh && (
        <div className="stack" style={{ gap: 10 }}>
          <Seg<Mode> value={mode} onChange={setMode} options={[{ value: "link", label: "Paste a link" }, { value: "list", label: "Paste a list or file" }]} />
          {mode === "link" ? (
            <form className="row" style={{ gap: 8 }} onSubmit={(e) => { e.preventDefault(); if (link.trim()) find({ link }); }}>
              <input className="input" style={{ flex: 1 }} value={link} onChange={(e) => setLink(e.target.value)} placeholder="https://open.spotify.com/playlist/…  ·  YouTube / YouTube Music  ·  Deezer" aria-label="Playlist link" spellCheck={false} autoFocus />
              <button className="btn primary" disabled={busy || !link.trim()}>{busy ? <Spinner /> : <I.search />} Find songs</button>
            </form>
          ) : (
            <div className="stack" style={{ gap: 8 }}>
              <textarea className="textarea mono" rows={7} value={text} onChange={(e) => setText(e.target.value)} placeholder={"One song per line, e.g.\nThe Beatles - Hey Jude\nBlinding Lights by The Weeknd\n\nAlso works: a CSV export (e.g. from Exportify) or an .m3u playlist."} aria-label="Song list" spellCheck={false} />
              <div className="row" style={{ gap: 8 }}>
                <input ref={file} type="file" accept=".m3u,.m3u8,.csv,.txt" hidden onChange={(e) => readFile(e.target.files?.[0])} />
                <button className="btn ghost sm" onClick={() => file.current?.click()}>Choose a file…</button>
                <span className="small dim" style={{ marginRight: "auto" }}>.m3u, .m3u8, .csv or .txt</span>
                <button className="btn primary" disabled={busy || !text.trim()} onClick={() => find({ text })}>{busy ? <Spinner /> : <I.search />} Find songs</button>
              </div>
            </div>
          )}
          <div className="small dim">Works with public Spotify playlists and albums, YouTube and YouTube Music playlists, and Deezer playlists and albums.</div>
        </div>
      )}

      {busy && !preview && refresh && <div className="row small dim" style={{ gap: 8 }}><Spinner /> Reading the playlist…</div>}
      {error && <div className="mt-2"><ErrorNote message={error} onRetry={() => find(mode === "link" || refresh ? { link } : { text })} /></div>}

      {preview && (
        <div className="stack mt-2" style={{ gap: 12 }}>
          <div className="import-summary">
            <div className="big">{preview.matched}<span className="dim"> / {preview.items.length}</span></div>
            <div>
              <div>songs from <b>{preview.name}</b> are in your library{preview.items.some((it) => it.track_id != null && m.byId.get(it.track_id)?.source === "youtube") ? " (some as streaming songs)" : ""}</div>
              <div className="small dim">{preview.source === "List" ? "From your list" : `From ${preview.source}`} · {preview.items.length - preview.matched} missing{preview.matched > 0 ? " · untick any match that's wrong" : ""}</div>
            </div>
          </div>
          {preview.truncated && <Note tone="warning">{preview.source} only shows the first {preview.items.length} songs of long playlists to apps that aren't signed in. For the full list, export it as CSV (e.g. with Exportify) and paste that instead.</Note>}
          {preview.matched === 0 && <Note>None of these songs are in your library yet. You can still save it: the list is remembered, and songs are filled in as you add them to your music folders.</Note>}

          <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
            <Seg<Filter> value={filter} onChange={setFilter} options={[{ value: "all", label: `All ${preview.items.length}` }, { value: "found", label: `In library ${preview.matched}` }, { value: "missing", label: `Missing ${missing.length}` }]} />
            {missing.length > 0 && <button className="btn ghost sm" onClick={() => copyMissing(missing, toast)}>Copy missing list</button>}
          </div>
          <div className="import-list" role="list">
            {shown.map(({ it, i }) => {
              const local = it.track_id != null ? m.byId.get(it.track_id) : undefined;
              return (
                <div key={i} className={"import-row" + (local ? (skip.has(i) ? " skipped" : " found") : " missing")} role="listitem">
                  {local ? (
                    <input type="checkbox" className="check" checked={!skip.has(i)} aria-label={`Include ${it.title}`} onChange={() => setSkip((s) => { const n = new Set(s); n.has(i) ? n.delete(i) : n.add(i); return n; })} />
                  ) : (
                    <span className="import-dot" aria-hidden />
                  )}
                  <div className="ir-body">
                    <div className="truncate">{it.title}<span className="dim"> · {it.artist || "unknown artist"}</span></div>
                    {local ? (
                      <div className="small dim truncate">
                        {local.source === "youtube" ? "Streams from YouTube · " : it.confidence === "likely" ? "Probably " : ""}→ {local.title} · {local.artist} · {fmtTime(local.duration_ms)}
                      </div>
                    ) : (
                      <div className="small dim">Not in your library</div>
                    )}
                  </div>
                  {it.duration_ms ? <span className="small dim">{fmtTime(it.duration_ms)}</span> : null}
                </div>
              );
            })}
          </div>

          {missing.length > 0 && (streaming?.enabled ? (
            <label className="row small" style={{ gap: 8, alignItems: "center" }}>
              <input type="checkbox" className="check" checked={stream} onChange={(e) => setStream(e.target.checked)} />
              Stream the {missing.length} missing song{missing.length === 1 ? "" : "s"} from YouTube
              <span className="dim">(found when you save; takes a few seconds per 4 songs)</span>
            </label>
          ) : (
            <div className="small dim">To play the missing songs without downloading them, turn on streaming in Music › Sources.</div>
          ))}
          {streamProgress && <div className="row small" style={{ gap: 8 }}><Spinner /> {streamProgress}</div>}
          <div className="row" style={{ gap: 8, alignItems: "flex-end" }}>
            <div className="field" style={{ flex: 1 }}>
              <label htmlFor="imp-name">Playlist name</label>
              <input id="imp-name" className="input" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <button className="btn primary" disabled={busy || !name.trim()} onClick={save}>
              {busy ? <Spinner /> : null} {refresh ? `Update playlist (${chosen} songs)` : `Create playlist (${chosen} songs)`}
            </button>
          </div>
        </div>
      )}
    </Panel>
  );
}

/** On an imported playlist: where it came from, songs still missing, and newly found ones. */
export function ImportedInfo({ playlist, onRefresh }: { playlist: Playlist; onRefresh: (url: string) => void }) {
  const m = useMusic();
  const { toast } = useApp();
  const [info, setInfo] = useState<ImportInfo | null>(null);
  const [open, setOpen] = useState(false);
  const streaming = useStreamStatus();
  const [streamBusy, setStreamBusy] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    music.importInfo(playlist.id).then((i) => live && setInfo(i)).catch(() => {});
    return () => {
      live = false;
    };
  }, [playlist.id, playlist.tracks.length, m.tracks.length]);

  if (!info) return null;
  const found = info.missing.filter((x) => x.track_id != null && m.byId.has(x.track_id));
  const still = info.missing.filter((x) => x.track_id == null);

  async function streamMissing() {
    setStreamBusy(`Finding songs on YouTube… 0 of ${still.length}`);
    try {
      const res = await streamSongs(still, (p) => setStreamBusy(`Finding songs on YouTube… ${p.done} of ${p.total} (${p.found} found)`));
      const ids = res.map((r) => r.track_id).filter((x): x is number => x != null);
      await m.reload();
      if (ids.length) {
        await music.savePlaylist({ ...playlist, tracks: [...playlist.tracks, ...ids] });
        await m.reloadPlaylists();
      }
      toast({ tone: ids.length ? "good" : "warning", title: ids.length ? `Added ${ids.length} streaming song${ids.length === 1 ? "" : "s"}` : "None of them were found on YouTube", detail: ids.length < still.length ? `${still.length - ids.length} couldn't be matched to an upload of the right length and version.` : undefined });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't stream them", detail: errorMessage(e) });
    } finally {
      setStreamBusy(null);
    }
  }

  async function addFound() {
    try {
      await music.savePlaylist({ ...playlist, tracks: [...playlist.tracks, ...found.map((x) => x.track_id!)] });
      await m.reloadPlaylists();
      toast({ tone: "good", title: `Added ${found.length} song${found.length === 1 ? "" : "s"}` });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't add them", detail: errorMessage(e) });
    }
  }

  return (
    <div className="import-info">
      <div className="row" style={{ gap: 10, flexWrap: "wrap", alignItems: "center" }}>
        <I.music />
        <span>
          Imported from {info.source === "List" ? "a list" : info.source} · {info.total - info.missing.length} of {info.total} songs in your library
        </span>
        <span className="row" style={{ gap: 6, marginLeft: "auto" }}>
          {info.url && <button className="btn ghost sm" onClick={() => music.importOpen(playlist.id).catch((e) => toast({ tone: "critical", title: "Couldn't open it", detail: errorMessage(e) }))}>Open original</button>}
          {info.url && <button className="btn ghost sm" onClick={() => onRefresh(info.url!)}><I.refresh /> Update from link</button>}
          {still.length > 0 && streaming?.enabled && <button className="btn sm" disabled={!!streamBusy} onClick={streamMissing}>{streamBusy ? <Spinner /> : <I.play />} Stream missing ({still.length})</button>}
          {still.length > 0 && <button className="btn ghost sm" onClick={() => setOpen((o) => !o)}>{open ? "Hide" : "Show"} missing ({still.length})</button>}
        </span>
      </div>
      {streamBusy && <div className="row small mt-2" style={{ gap: 8 }}><Spinner /> {streamBusy}</div>}
      {found.length > 0 && (
        <div className="row mt-2" style={{ gap: 10, alignItems: "center" }}>
          <span className="small">{found.length} song{found.length === 1 ? " that was" : "s that were"} missing {found.length === 1 ? "is" : "are"} now in your library.</span>
          <button className="btn primary sm" onClick={addFound}>Add {found.length === 1 ? "it" : "them"}</button>
        </div>
      )}
      {open && still.length > 0 && (
        <div className="stack mt-2" style={{ gap: 6 }}>
          <div className="row" style={{ gap: 8 }}>
            <span className="small dim">Add these to your music folders and they'll be offered here.</span>
            <button className="btn ghost sm" onClick={() => copyMissing(still, toast)}>Copy list</button>
          </div>
          <div className="import-list compact">
            {still.map((x, i) => (
              <div key={i} className="import-row missing"><span className="import-dot" aria-hidden /><div className="ir-body truncate">{x.title}<span className="dim"> · {x.artist || "unknown artist"}</span></div></div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
