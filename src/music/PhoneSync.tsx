// Music › Sources › Sync with iPhone: the QR code the Nocturn Music iPhone
// app scans once, then playlists and liked songs sync over the home Wi-Fi.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { I } from "../components/Icons";
import { Note, Panel, Spinner, Toggle } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/app";
import { relative } from "../lib/format";

type LastSync = { device: string; at: string; playlists: number; songs: number };
type SyncInfo = { enabled: boolean; running: boolean; address: string | null; port: number; name: string; qrSvg: string | null; last: LastSync | null };

export function PhoneSyncPanel() {
  const { toast } = useApp();
  const [info, setInfo] = useState<SyncInfo | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    invoke<SyncInfo>("phone_sync_info").then(setInfo).catch(() => {});
    const un = listen<LastSync>("phone-sync", () => invoke<SyncInfo>("phone_sync_info").then(setInfo).catch(() => {}));
    return () => {
      un.then((f) => f());
    };
  }, []);

  async function set(on: boolean) {
    setBusy(true);
    try {
      setInfo(await invoke<SyncInfo>("phone_sync_set_enabled", { enabled: on }));
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't change iPhone sync", detail: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  async function newCode() {
    try {
      setInfo(await invoke<SyncInfo>("phone_sync_reset_key"));
      toast({ tone: "good", title: "New code made", detail: "Phones paired before need to scan it again." });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't make a new code", detail: errorMessage(e) });
    }
  }

  return (
    <Panel title="Sync with iPhone" sub="Your playlists and liked songs, on Nocturn Music for iPhone and here. Over your own Wi-Fi: no account, nothing online.">
      <div className="row" style={{ gap: 12, alignItems: "center" }}>
        <div style={{ flex: 1 }}>
          <div>Let my iPhone sync</div>
          <div className="small dim">Only devices on your home network that scanned the code can connect.</div>
        </div>
        {busy ? <Spinner /> : <Toggle on={!!info?.enabled} onChange={set} label="Let my iPhone sync" />}
      </div>
      {info?.enabled && (
        <div className="row mt-2" style={{ gap: 18, alignItems: "center", flexWrap: "wrap" }}>
          {info.qrSvg ? (
            <div className="phone-sync-qr" style={{ width: 200, height: 200, borderRadius: 14, overflow: "hidden", background: "#fff", flex: "none" }} dangerouslySetInnerHTML={{ __html: info.qrSvg.replace(/<\?xml[^>]*\?>/, "").replace(/width="\d+"/, 'width="200"').replace(/height="\d+"/, 'height="200"') }} />
          ) : (
            <Note tone="warning">This PC isn't connected to a network, so there's nothing to scan. Connect to your Wi-Fi or router and reopen this page.</Note>
          )}
          <ol className="small" style={{ margin: 0, paddingLeft: 18, lineHeight: 1.7, flex: 1, minWidth: 220 }}>
            <li>Make sure your iPhone is on the same Wi-Fi as this PC.</li>
            <li>In Nocturn Music on the iPhone: Library › Settings › <b>PC Sync</b> › <b>Pair with PC</b>.</li>
            <li>Point the camera at this code. That's it, you only do this once.</li>
            <li>If Windows asks whether NOCTURN may use your network, choose <b>Allow</b> (private networks).</li>
          </ol>
        </div>
      )}
      {info?.enabled && !info.running && <Note tone="warning">The sync service isn't running (port {info.port} may be in use). Switch it off and on again, or restart NOCTURN.</Note>}
      {info?.last && (
        <div className="small dim mt-2">
          Last sync: {info.last.device}, {relative(info.last.at)} · {info.last.playlists} playlist{info.last.playlists === 1 ? "" : "s"}, {info.last.songs} songs from the phone
        </div>
      )}
      {info?.enabled && (
        <div className="row mt-2" style={{ gap: 8 }}>
          <button className="btn ghost sm" onClick={newCode}>
            <I.refresh /> Make a new code
          </button>
          <span className="small dim" style={{ alignSelf: "center" }}>
            {info.name} · {info.address ?? "no network"}:{info.port}
          </span>
        </div>
      )}
    </Panel>
  );
}
