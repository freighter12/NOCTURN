// Unofficial YouTube streaming: the switch in Music › Sources, and a helper
// that turns missing songs into streaming tracks (used by playlist import).

import { useEffect, useState } from "react";
import { I } from "../components/Icons";
import { Note, Panel, Spinner, Toggle } from "../components/ui";
import { errorMessage, musicStream, type StreamInstallProgress, type StreamResolveProgress, type StreamResolved, type StreamStatus, type StreamWanted } from "../lib/api";
import { useApp } from "../lib/app";
import { relative } from "../lib/format";
import { useMusic } from "./MusicProvider";

let shared: StreamStatus | null = null;
const listeners = new Set<(s: StreamStatus) => void>();
const publish = (s: StreamStatus) => {
  shared = s;
  listeners.forEach((f) => f(s));
};

/** Current streaming status, shared across components. */
export function useStreamStatus() {
  const [s, setS] = useState<StreamStatus | null>(shared);
  useEffect(() => {
    listeners.add(setS);
    musicStream.status().then(publish).catch(() => {});
    return () => {
      listeners.delete(setS);
    };
  }, []);
  return s;
}

/** Find YouTube uploads for songs; returns results in the same order. */
export async function streamSongs(items: StreamWanted[], onProgress?: (p: StreamResolveProgress) => void): Promise<StreamResolved[]> {
  const r = await musicStream.resolve(items, onProgress);
  musicStream.status().then(publish).catch(() => {});
  return r;
}

export function StreamingPanel() {
  const { toast } = useApp();
  const m = useMusic();
  const status = useStreamStatus();
  const [busy, setBusy] = useState<StreamInstallProgress | null>(null);
  const [checking, setChecking] = useState(false);
  const [confirmOff, setConfirmOff] = useState(false);

  async function enable() {
    setBusy({ stage: "Starting", fraction: null });
    try {
      publish(await musicStream.enable(setBusy));
      toast({ tone: "good", title: "Streaming is on", detail: "Songs that aren't in your library can now play from YouTube." });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't set up streaming", detail: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  }
  async function disable(remove: boolean) {
    try {
      publish(await musicStream.disable(remove));
      setConfirmOff(false);
      toast({ tone: "good", title: remove ? "Streaming turned off and tools removed" : "Streaming turned off" });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't turn streaming off", detail: errorMessage(e) });
    }
  }
  async function prune() {
    try {
      const n = await musicStream.prune();
      publish(await musicStream.status());
      await m.reload();
      toast({ tone: "good", title: `Removed ${n} streaming song${n === 1 ? "" : "s"}` });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't remove them", detail: errorMessage(e) });
    }
  }
  async function update() {
    setChecking(true);
    try {
      const changed = await musicStream.update();
      publish(await musicStream.status());
      toast({ tone: "good", title: changed ? "yt-dlp updated" : "yt-dlp is up to date" });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't check for a yt-dlp update", detail: errorMessage(e) });
    } finally {
      setChecking(false);
    }
  }

  const on = !!status?.enabled;
  return (
    <Panel title="Stream songs you don't have" sub="Play songs from imported playlists that aren't in your music folders, straight from YouTube." lit={on}>
      <div className="list">
        <div className="list-item" style={{ alignItems: "center" }}>
          <div className="body">
            <div className="title">YouTube streaming (unofficial)</div>
            <div className="desc">
              {on
                ? `On · yt-dlp ${status?.ytdlp_version ?? "?"}${status?.checked_at ? `, checked for updates ${relative(status.checked_at)}` : ""} · ${status?.streaming_tracks ?? 0} streaming songs`
                : status?.installed
                  ? "Off · the tools are still downloaded"
                  : "Off · turning it on downloads yt-dlp and Deno (about 65 MB) from their official GitHub releases, checked against their published checksums."}
            </div>
          </div>
          {busy ? <Spinner /> : <Toggle on={on} label="YouTube streaming" onChange={(v) => (v ? enable() : setConfirmOff(true))} />}
        </div>
      </div>
      {busy && (
        <div className="row small mt-2" style={{ gap: 8 }}>
          <Spinner /> {busy.stage}
          {busy.fraction != null && <span className="dim">{Math.round(busy.fraction * 100)}%</span>}
        </div>
      )}
      {confirmOff && (
        <div className="row mt-2" style={{ gap: 8, flexWrap: "wrap", alignItems: "center" }}>
          <span className="small">Streaming songs stay in your playlists but won't play until it's back on.</span>
          <button className="btn sm" onClick={() => disable(false)}>Turn off</button>
          <button className="btn ghost sm" onClick={() => disable(true)}>Turn off and remove the tools</button>
          <button className="btn ghost sm" onClick={() => setConfirmOff(false)}>Cancel</button>
        </div>
      )}
      {on && (
        <div className="row mt-2" style={{ gap: 8 }}>
          <button className="btn ghost sm" disabled={checking} onClick={update}>{checking ? <Spinner /> : <I.refresh />} Check for a yt-dlp update</button>
          <span className="small dim">Checked automatically once a day — YouTube changes often break older versions.</span>
        </div>
      )}
      {!!status?.unused_tracks && (
        <div className="row mt-2" style={{ gap: 8, alignItems: "center" }}>
          <span className="small dim">{status.unused_tracks} streaming song{status.unused_tracks === 1 ? " isn't" : "s aren't"} in any playlist or liked.</span>
          <button className="btn ghost sm" onClick={prune}>Remove {status.unused_tracks === 1 ? "it" : "them"}</button>
        </div>
      )}
      <div className="mt-2">
        <Note tone="warning">
          This plays audio straight from YouTube without its player or ads, which is against YouTube's terms. It's for your own listening, can stop working whenever YouTube changes something (NOCTURN updates yt-dlp to keep up), and a song can take about 8 seconds to start (the next one is loaded early). Songs in your music folders always play from your files.
        </Note>
      </div>
    </Panel>
  );
}
