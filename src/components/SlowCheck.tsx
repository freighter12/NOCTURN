// "Why is my PC slow right now?" — a 5-second look, then plain-English
// findings with one-click fixes (closing an app asks first; changes go through
// the approval dialog).

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { api, errorMessage } from "../lib/api";
import { useApp, type PageId } from "../lib/app";
import { I } from "./Icons";
import { Spinner } from "./ui";

type Action =
  | { kind: "close_app"; name: string; label: string }
  | { kind: "open_page"; page: string; label: string }
  | { kind: "apply_tweak"; id: string; label: string };

interface Finding {
  severity: "high" | "medium" | "info";
  title: string;
  detail: string;
  actions: Action[];
}

interface Report {
  summary: { seconds: number; cpu_pct: number; memory_pct: number; disk_busy_pct: number; download_bps: number; upload_bps: number; gpu_pct: number | null; gpu_temp_c: number | null; uptime_days: number; game: string | null };
  findings: Finding[];
  verdict: string;
}

const SECONDS = 5;

export const openSlowCheck = () => window.dispatchEvent(new Event("nocturn:slowcheck"));

const mbit = (bps: number) => `${((bps * 8) / 1_000_000).toFixed(bps * 8 >= 10_000_000 ? 0 : 1)} Mbit/s`;

export function SlowCheck() {
  const { navigate, requestApply, toast } = useApp();
  const [open, setOpen] = useState(false);
  const [report, setReport] = useState<Report | null>(null);
  const [left, setLeft] = useState(SECONDS);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [closing, setClosing] = useState<string | null>(null);

  const run = () => {
    setReport(null);
    setConfirm(null);
    setLeft(SECONDS);
    const started = Date.now();
    const iv = window.setInterval(() => setLeft(Math.max(0, SECONDS - Math.floor((Date.now() - started) / 1000))), 250);
    invoke<Report>("slow_check", { seconds: SECONDS })
      .then(setReport)
      .catch((e) => {
        toast({ tone: "critical", title: "Couldn't check", detail: errorMessage(e) });
        setOpen(false);
      })
      .finally(() => window.clearInterval(iv));
  };

  useEffect(() => {
    const on = () => {
      setOpen(true);
      run();
    };
    window.addEventListener("nocturn:slowcheck", on);
    return () => window.removeEventListener("nocturn:slowcheck", on);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!open) return null;
  const close = () => setOpen(false);

  const act = async (a: Action) => {
    if (a.kind === "open_page") {
      close();
      navigate(a.page as PageId);
    } else if (a.kind === "apply_tweak") {
      close();
      requestApply({ tweakIds: [a.id], source: "user", label: a.label, title: a.label });
    } else if (confirm !== a.name) {
      setConfirm(a.name);
    } else {
      setClosing(a.name);
      try {
        const r = await api.closeApps([a.name], "user");
        const ok = r.every((x) => x.failed === 0 && !x.error) && r.some((x) => x.ended > 0);
        toast({ tone: ok ? "good" : "warning", title: ok ? `${a.label.replace(/^Close /, "")} closed` : `Couldn't close everything`, detail: ok ? "Check again to see the difference." : undefined });
      } catch (e) {
        toast({ tone: "critical", title: "Couldn't close it", detail: errorMessage(e) });
      } finally {
        setClosing(null);
        setConfirm(null);
      }
    }
  };

  const s = report?.summary;
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal slowcheck" role="dialog" aria-modal="true" aria-labelledby="slow-title">
        <div className="modal-head">
          <h2 id="slow-title">Why is my PC slow?</h2>
          <p>{report ? report.verdict : `Watching what your PC is doing… ${left > 0 ? `${left} s` : "almost done"}`}</p>
        </div>
        <div className="modal-body">
          {!report ? (
            <div className="slow-watch">
              <div className="slow-ring" style={{ ["--k" as string]: `${((SECONDS - left) / SECONDS) * 100}%` }}>
                <Spinner lg />
              </div>
              <div className="small dim">Measuring the processor, memory, disks and network, and which apps are using them.</div>
            </div>
          ) : (
            <div className="stack" style={{ gap: 14 }}>
              <div className="slow-chips">
                <span className={`chip ${s!.cpu_pct >= 85 ? "hot" : ""}`}>Processor {s!.cpu_pct.toFixed(0)}%</span>
                <span className={`chip ${s!.memory_pct >= 85 ? "hot" : ""}`}>Memory {s!.memory_pct.toFixed(0)}%</span>
                <span className={`chip ${s!.disk_busy_pct >= 85 ? "hot" : ""}`}>Disk busy {s!.disk_busy_pct.toFixed(0)}%</span>
                <span className="chip">Down {mbit(s!.download_bps)}</span>
                {s!.gpu_pct != null && <span className="chip">Graphics {s!.gpu_pct.toFixed(0)}%{s!.gpu_temp_c != null ? ` · ${s!.gpu_temp_c.toFixed(0)} °C` : ""}</span>}
                {s!.game && <span className="chip">Playing {s!.game}</span>}
              </div>
              {report.findings.length === 0 && (
                <div className="slow-ok">
                  <I.check />
                  <div>
                    <b>Nothing is slowing your PC down right now.</b>
                    <div className="small dim">If it felt slow a moment ago, run this again while it's happening — the cause is often brief.</div>
                  </div>
                </div>
              )}
              {report.findings.map((f, i) => (
                <div key={i} className={`slow-finding ${f.severity}`}>
                  <span className="slow-dot" aria-hidden />
                  <div className="slow-body">
                    <div className="title">{f.title}</div>
                    <div className="desc">{f.detail}</div>
                    {f.actions.length > 0 && (
                      <div className="row" style={{ gap: 8, marginTop: 8, flexWrap: "wrap" }}>
                        {f.actions.map((a, j) => (
                          <button key={j} className={`btn sm ${a.kind === "close_app" && confirm === a.name ? "danger" : ""}`} disabled={closing !== null} onClick={() => act(a)}>
                            {closing === (a.kind === "close_app" ? a.name : "") ? <Spinner /> : null}
                            {a.kind === "close_app" && confirm === a.name ? `Yes, ${a.label.replace(/^Close/, "close")} (unsaved work may be lost)` : a.label}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="modal-foot">
          <button className="btn ghost" onClick={close}>Close</button>
          {report && <button className="btn" onClick={run}><I.refresh /> Check again</button>}
        </div>
      </div>
    </div>
  );
}
