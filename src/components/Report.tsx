// "Report a problem": describe what happened, review the report (personal
// details already removed), then save it to the Desktop or copy it.

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/app";
import { recentErrors } from "../lib/errors";
import { I } from "./Icons";
import { Spinner } from "./ui";

/** Open the dialog from anywhere. */
export const openReport = () => window.dispatchEvent(new Event("nocturn:report"));

export function ReportDialog() {
  const { page, toast } = useApp();
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState("");
  const [logs, setLogs] = useState(true);
  const [text, setText] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const on = () => {
      setOpen(true);
      setText(null);
    };
    window.addEventListener("nocturn:report", on);
    return () => window.removeEventListener("nocturn:report", on);
  }, []);

  if (!open) return null;
  const close = () => setOpen(false);

  async function preview() {
    setBusy(true);
    try {
      setText(await invoke<string>("report_preview", { input: { note, page_errors: recentErrors(), page, include_logs: logs } }));
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't put the report together", detail: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (!text) return;
    setBusy(true);
    try {
      const path = await invoke<string>("report_save", { text });
      toast({ tone: "good", title: "Report saved", detail: `${path.split("\\").pop()} — on your Desktop, ready to send.` });
      setNote("");
      close();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save the report", detail: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  async function copy() {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      toast({ tone: "good", title: "Report copied", detail: "Paste it into a message to whoever helps you with NOCTURN." });
    } catch {
      toast({ tone: "warning", title: "Couldn't copy", detail: "Save it to the Desktop instead." });
    }
  }

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && !busy && close()}>
      <div className="modal narrow report" role="dialog" aria-modal="true" aria-labelledby="report-title">
        <div className="modal-head">
          <h2 id="report-title">Report a problem</h2>
          <p>Makes one text file you can send to whoever helps you with NOCTURN. You'll see everything in it first.</p>
        </div>
        <div className="modal-body stack" style={{ gap: 12 }}>
          {text === null ? (
            <>
              <div className="field">
                <label htmlFor="report-note">What happened? What were you doing?</label>
                <textarea id="report-note" className="textarea" rows={5} maxLength={4000} value={note} onChange={(e) => setNote(e.target.value)} placeholder="e.g. I clicked Import playlist and nothing happened. Or: Minecraft isn't in my games." autoFocus />
              </div>
              <label className="row small" style={{ gap: 8, alignItems: "center" }}>
                <input type="checkbox" className="check" checked={logs} onChange={(e) => setLogs(e.target.checked)} />
                Include recent warnings and errors from NOCTURN's log
              </label>
              <div className="small dim">
                Included: NOCTURN's version, your Windows version, a hardware summary (CPU, graphics, RAM), relevant settings, and problems NOCTURN noticed. Never included: your files, API keys or what you play or listen to. Your user name, PC name, email and IP addresses are removed.
              </div>
            </>
          ) : (
            <>
              <div className="small dim">This is the whole report. Personal details are already replaced with &lt;user&gt;, &lt;email&gt;, &lt;ip&gt; and &lt;key&gt;.</div>
              <textarea className="textarea mono report-text" readOnly value={text} rows={16} aria-label="Report text" />
            </>
          )}
        </div>
        <div className="modal-foot">
          {text === null ? (
            <>
              <button className="btn ghost" onClick={close} disabled={busy}>Cancel</button>
              <button className="btn primary" onClick={preview} disabled={busy}>{busy ? <Spinner /> : null} Review report</button>
            </>
          ) : (
            <>
              <button className="btn ghost" onClick={() => setText(null)} disabled={busy}>Back</button>
              <span className="left" />
              <button className="btn" onClick={copy} disabled={busy}>Copy</button>
              <button className="btn primary" onClick={save} disabled={busy}>{busy ? <Spinner /> : <I.check />} Save to Desktop</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
