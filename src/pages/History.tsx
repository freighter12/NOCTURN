// Transaction history (every change NOCTURN made, with originals and
// rollback) and the audit log (every security-relevant event).

import { useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Seg, Spinner, StatusTag } from "../components/ui";
import { api, errorMessage, type ChangeRow, type Progress, type RollbackReport } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { date, relative } from "../lib/format";

type Tab = "changes" | "audit";

export function History() {
  const [tab, setTab] = useState<Tab>("changes");
  return (
    <div className="page">
      <PageHead
        eyebrow="History"
        title="Change history"
        desc="Every change NOCTURN makes is written to a journal before it happens, with the original value, so it can be rolled back. The audit log records approvals, elevation requests and AI activity."
        actions={<Seg<Tab> value={tab} onChange={setTab} options={[{ value: "changes", label: "Changes" }, { value: "audit", label: "Audit log" }]} />}
      />
      {tab === "changes" ? <Transactions /> : <Audit />}
    </div>
  );
}

const ROLLBACKABLE = new Set(["completed", "partial", "interrupted", "partially_rolled_back"]);

function sourceLabel(s: string) {
  if (s === "ai") return "Approved from AI plan";
  if (s.startsWith("profile:")) return "Profile";
  if (s === "auto") return "Automatic (app profile)";
  return "Manual";
}

function Transactions() {
  const { dataVersion, bumpData, toast } = useApp();
  const list = useLoad(() => api.history(200), [dataVersion]);
  const [sel, setSel] = useState<string | null>(null);
  const current = sel ?? list.data?.[0]?.id ?? null;

  if (list.loading && !list.data) return <Loading label="Reading the change journal…" />;
  if (list.error) return <ErrorNote message={list.error} onRetry={list.reload} />;
  if (!list.data?.length)
    return (
      <Panel>
        <Empty title="No changes yet">When you apply an optimisation, profile, power plan or startup change, it appears here with its original values.</Empty>
      </Panel>
    );

  return (
    <div className="grid" style={{ gridTemplateColumns: "minmax(0, 340px) minmax(0, 1fr)", alignItems: "start" }}>
      <section className="panel flush" style={{ maxHeight: "calc(100vh - 210px)", overflow: "auto" }}>
        {list.data.map((t) => (
          <button
            key={t.id}
            className="list-item"
            onClick={() => setSel(t.id)}
            style={{
              width: "100%", textAlign: "left", border: 0, borderTop: "1px solid var(--line)", padding: "12px 16px", cursor: "default", color: "inherit",
              background: current === t.id ? "rgba(255,255,255,.045)" : "transparent",
            }}
          >
            <div className="body">
              <div className="title truncate">{t.label}</div>
              <div className="desc">
                {relative(t.created_at)} · {sourceLabel(t.source)}
              </div>
            </div>
            <StatusTag status={t.status} />
          </button>
        ))}
      </section>
      {current && <TxDetail key={current} id={current} onChanged={() => { bumpData(); list.reload(); }} toast={toast} />}
    </div>
  );
}

function TxDetail({ id, onChanged, toast }: { id: string; onChanged: () => void; toast: ReturnType<typeof useApp>["toast"] }) {
  const tx = useLoad(() => api.transaction(id), [id]);
  const [confirm, setConfirm] = useState(false);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [report, setReport] = useState<RollbackReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (tx.loading && !tx.data) return <Panel><Loading label="Loading transaction…" /></Panel>;
  if (tx.error || !tx.data) return <ErrorNote message={tx.error ?? "Not found"} onRetry={tx.reload} />;
  const { summary: s, changes } = tx.data;
  const reversibleApplied = changes.filter((c) => c.status === "applied" && c.original && c.original.type !== "nothing").length;
  const canRollback = ROLLBACKABLE.has(s.status) && changes.some((c) => c.status === "applied");

  async function rollback() {
    setRunning(true);
    setError(null);
    setConfirm(false);
    try {
      const r = await api.rollback(id, setProgress);
      setReport(r);
      const failed = r.results.filter((x) => x.status === "rollback_failed").length;
      toast({ tone: failed ? "warning" : "good", title: failed ? "Rollback partly completed" : "Rolled back", detail: `${r.results.filter((x) => x.status === "rolled_back").length} restored · ${failed} failed` });
      tx.reload();
      onChanged();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRunning(false);
      setProgress(null);
    }
  }

  return (
    <Panel
      title={s.label}
      sub={`${date(s.created_at, true)} · ${sourceLabel(s.source)} · ${s.applied} applied${s.failed ? ` · ${s.failed} failed` : ""}${s.rolled_back ? ` · ${s.rolled_back} rolled back` : ""}`}
      actions={
        canRollback &&
        (confirm ? (
          <span className="row" style={{ gap: 6 }}>
            <button className="btn ghost sm" onClick={() => setConfirm(false)}>Cancel</button>
            <button className="btn primary sm" onClick={rollback}>Restore originals</button>
          </span>
        ) : (
          <button className="btn sm" disabled={running} onClick={() => setConfirm(true)}>
            {running ? <Spinner /> : <I.undo />} Roll back
          </button>
        ))
      }
    >
      <div className="stack" style={{ gap: 12 }}>
        <div className="row wrap small" style={{ gap: 8 }}>
          <StatusTag status={s.status} />
          <span className="dim mono">{s.id}</span>
        </div>
        {confirm && (
          <Note tone="warning">
            This restores the {reversibleApplied || "recorded"} original value(s) below. One-time actions (like clearing caches) can't be undone and will be skipped. Some settings need administrator approval or a restart to revert.
          </Note>
        )}
        {s.restore_point && <Note tone="good">Windows restore point: {s.restore_point}</Note>}
        {s.backup_path && (
          <div className="small muted">
            Settings backup: <span className="mono selectable">{s.backup_path}</span>
          </div>
        )}
        {running && (
          <div className="stack" style={{ gap: 6 }}>
            <div className="row between small">
              <span>{progress?.message ?? "Restoring…"}</span>
              <span className="dim num">{progress ? `${progress.done}/${progress.total}` : ""}</span>
            </div>
            <div className={`progress ${progress?.total ? "" : "indeterminate"}`}>
              <span style={{ width: progress?.total ? `${(progress.done / progress.total) * 100}%` : "30%" }} />
            </div>
          </div>
        )}
        {error && <ErrorNote message={error} />}
        {report && report.notes.map((n, i) => <div key={i} className="small muted">{n}</div>)}

        <div>
          <div className="change-row small dim" style={{ borderTop: 0, textTransform: "uppercase", letterSpacing: ".06em", fontSize: 10.5 }}>
            <div>Setting</div>
            <div className="val" style={{ color: "inherit" }}>Original</div>
            <div />
            <div style={{ textAlign: "right" }}>Status</div>
          </div>
          {changes.map((c) => (
            <ChangeLine key={c.id} c={c} />
          ))}
        </div>
      </div>
    </Panel>
  );
}

/** Render a recorded original value (tweaks::Original) for humans. */
function describe(o: Record<string, unknown> | null): string {
  if (!o) return "—";
  if (o.type === "nothing") return "one-time";
  if (o.type === "power_plan") return `plan ${String(o.scheme).slice(0, 8)}…`;
  if (o.type === "registry") {
    const v = o.value as { vtype: number; bytes: number[] } | null;
    if (!v) return "not set";
    const b = v.bytes;
    if (v.vtype === 4 && b.length >= 4) return String((b[0] | (b[1] << 8) | (b[2] << 16) | (b[3] << 24)) >>> 0);
    if (v.vtype === 1 || v.vtype === 2) {
      let str = "";
      for (let i = 0; i + 1 < b.length; i += 2) {
        const c = b[i] | (b[i + 1] << 8);
        if (!c) break;
        str += String.fromCharCode(c);
      }
      return str ? `"${str}"` : "(empty)";
    }
    return b.slice(0, 12).map((x) => x.toString(16).padStart(2, "0")).join(" ") + (b.length > 12 ? " …" : "");
  }
  return JSON.stringify(o).slice(0, 80);
}

function ChangeLine({ c }: { c: ChangeRow }) {
  return (
    <div className="change-row">
      <div style={{ minWidth: 0 }}>
        <div className="target selectable">{c.target}</div>
        <div className="small dim">
          {c.tweak_id}
          {c.applied_at ? ` · applied ${relative(c.applied_at)}` : ""}
          {c.rolled_back_at ? ` · reverted ${relative(c.rolled_back_at)}` : ""}
        </div>
        {c.error && <div className="small" style={{ color: "#ee8a8a" }}>{c.error}</div>}
      </div>
      <div className="val selectable" title={JSON.stringify(c.original)}>{describe(c.original)}</div>
      <div />
      <div style={{ textAlign: "right" }}>
        <StatusTag status={c.status} />
      </div>
    </div>
  );
}

function Audit() {
  const log = useLoad(() => api.listAudit(300), []);
  const [filter, setFilter] = useState("");
  if (log.loading && !log.data) return <Loading label="Reading the audit log…" />;
  if (log.error) return <ErrorNote message={log.error} onRetry={log.reload} />;
  const rows = (log.data ?? []).filter((e) => !filter || `${e.actor} ${e.event} ${JSON.stringify(e.detail)}`.toLowerCase().includes(filter.toLowerCase()));
  return (
    <section className="panel flush">
      <div className="panel-head" style={{ paddingBottom: 12 }}>
        <div>
          <h2>Audit log</h2>
          <div className="sub">Most recent first. Stored locally; nothing here is sent anywhere.</div>
        </div>
        <div className="row">
          <input className="input" style={{ width: 240, height: 30 }} placeholder="Filter events" value={filter} onChange={(e) => setFilter(e.target.value)} />
          <button className="btn sm icon" onClick={log.reload} aria-label="Refresh"><I.refresh /></button>
        </div>
      </div>
      {rows.length === 0 ? (
        <Empty title="No matching events" />
      ) : (
        <div className="table-wrap" style={{ maxHeight: "calc(100vh - 260px)" }}>
          <table className="table">
            <thead>
              <tr>
                <th style={{ width: 170 }}>Time</th>
                <th style={{ width: 80 }}>Level</th>
                <th style={{ width: 90 }}>Actor</th>
                <th>Event</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((e, i) => (
                <tr key={i}>
                  <td className="small muted num">{date(e.ts, true)}</td>
                  <td>
                    <span className={`tag ${e.level === "error" ? "critical" : e.level === "warn" || e.level === "warning" ? "warning" : ""}`}>{e.level}</span>
                  </td>
                  <td className="small">{e.actor}</td>
                  <td>
                    <div>{e.event}</div>
                    {e.detail != null && <div className="mono dim selectable" style={{ fontSize: 11, overflowWrap: "anywhere" }}>{JSON.stringify(e.detail).slice(0, 400)}</div>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

