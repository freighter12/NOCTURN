// Settings › Privacy: what Nocturn keeps (all on this PC), what leaves it and
// when, and controls to clear what the user doesn't want kept.

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { bytes } from "../lib/format";
import { I } from "./Icons";
import { ErrorNote, Loading, Panel } from "./ui";

interface Overview {
  data_dir: string;
  database_bytes: number;
  stored: { key: string; label: string; detail: string; count: number; clearable: boolean }[];
  outbound: { what: string; when: string; to: string }[];
}

export function PrivacyPanel() {
  const { toast } = useApp();
  const data = useLoad(() => invoke<Overview>("privacy_overview"), []);
  const [confirm, setConfirm] = useState<string | null>(null);

  async function clear(key: string, label: string) {
    try {
      await invoke("privacy_clear", { category: key });
      toast({ tone: "good", title: `${label} cleared` });
      setConfirm(null);
      data.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't clear", detail: errorMessage(e) });
    }
  }

  return (
    <Panel title="Privacy" sub="No ads, no trackers, no analytics. Everything below is stored only on this PC.">
      {data.loading && !data.data && <Loading />}
      {data.error && <ErrorNote message={data.error} onRetry={data.reload} />}
      {data.data && (
        <div className="stack" style={{ gap: 14 }}>
          <div>
            <div className="section-title" style={{ margin: "0 0 6px" }}>Leaves this PC</div>
            <div className="list">
              {data.data.outbound.map((o) => (
                <div key={o.what} className="list-item">
                  <div className="body">
                    <div className="title">{o.what}</div>
                    <div className="desc">{o.when}. To: {o.to}.</div>
                  </div>
                </div>
              ))}
            </div>
            <div className="small dim mt-1">Nothing else is sent anywhere. Connected services in your Library open in their own apps or your browser; Nocturn never signs in to them or reads their data.</div>
          </div>
          <div>
            <div className="section-title" style={{ margin: "0 0 6px" }}>Kept on this PC</div>
            <div className="list">
              {data.data.stored.map((s) => (
                <div key={s.key} className="list-item" style={{ alignItems: "center" }}>
                  <div className="body">
                    <div className="title">{s.label} <span className="dim small num">· {s.count.toLocaleString()}</span></div>
                    <div className="desc">{s.detail}</div>
                  </div>
                  {s.clearable ? (
                    confirm === s.key ? (
                      <span className="row" style={{ gap: 6 }}>
                        <button className="btn ghost sm" onClick={() => setConfirm(null)}>Cancel</button>
                        <button className="btn sm" onClick={() => clear(s.key, s.label)}>Clear</button>
                      </span>
                    ) : (
                      <button className="btn ghost sm" disabled={s.count === 0} onClick={() => setConfirm(s.key)}>Clear…</button>
                    )
                  ) : (
                    <span className="tag" title="Needed for undo and your protection"><I.lock /> Kept</span>
                  )}
                </div>
              ))}
            </div>
            <div className="small dim mt-1">
              Database: {bytes(data.data.database_bytes)} in <span className="mono selectable">{data.data.data_dir}</span>. API keys live in Windows Credential Manager, never in this database.
            </div>
          </div>
        </div>
      )}
    </Panel>
  );
}
