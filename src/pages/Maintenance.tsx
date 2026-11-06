// Scheduled maintenance: a fixed set of tasks NOCTURN runs while it's open,
// deferred during games, benchmarks and heavy CPU use, with a run history.

import { useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Spinner, Toggle } from "../components/ui";
import { api, errorMessage, type MaintenanceRun, type MaintenanceTask } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { date, relative, until } from "../lib/format";

const STATUS: Record<MaintenanceRun["status"], { cls: string; label: string }> = {
  ok: { cls: "good", label: "Done" },
  attention: { cls: "warning", label: "Needs attention" },
  failed: { cls: "critical", label: "Failed" },
  deferred: { cls: "", label: "Deferred" },
};
const EVERY: Record<number, string> = { 1: "Every day", 7: "Every week", 14: "Every 2 weeks", 30: "Every month" };

export function Maintenance() {
  const { toast } = useApp();
  const data = useLoad(() => api.maintenance(), []);
  const [running, setRunning] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);

  async function save(t: MaintenanceTask, enabled: boolean, days: number) {
    try {
      await api.setMaintenanceTask(t.id, enabled, days);
      data.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save", detail: errorMessage(e) });
    }
  }

  async function runNow(t: MaintenanceTask) {
    setConfirm(null);
    setRunning(t.id);
    try {
      const r = await api.runMaintenanceTask(t.id);
      toast({ tone: r.status === "ok" ? "good" : r.status === "attention" ? "warning" : "critical", title: `${t.title}: ${STATUS[r.status].label.toLowerCase()}`, detail: r.summary });
      data.reload();
    } catch (e) {
      toast({ tone: "critical", title: `${t.title} didn't run`, detail: errorMessage(e) });
    } finally {
      setRunning(null);
    }
  }

  if (data.loading && !data.data) return <div className="page"><Loading /></div>;
  if (data.error || !data.data) return <div className="page"><ErrorNote message={data.error ?? "No data"} onRetry={data.reload} /></div>;
  const { tasks, history } = data.data;
  const title = (id: string) => tasks.find((t) => t.id === id)?.title ?? id;

  return (
    <div className="page">
      <PageHead
        eyebrow="Maintenance"
        title="Scheduled maintenance"
        desc="Checks and clean-ups NOCTURN runs on a schedule while it's open. Anything due waits while you're gaming, benchmarking or the CPU is busy, then runs when the PC is quiet."
      />

      <section className="panel flush">
        <table className="table">
          <thead>
            <tr>
              <th>Task</th>
              <th style={{ width: 160 }}>How often</th>
              <th style={{ width: 150 }}>Last run</th>
              <th style={{ width: 70 }}>On</th>
              <th className="r" style={{ width: 140 }} />
            </tr>
          </thead>
          <tbody>
            {tasks.map((t) => (
              <tr key={t.id}>
                <td>
                  <div className="row" style={{ gap: 8 }}>
                    <span style={{ fontWeight: 500 }}>{t.title}</span>
                    {t.changes_system ? <span className="tag warning" title="This task deletes files">Changes files</span> : <span className="tag" title="Only reads information">Read-only</span>}
                  </div>
                  <div className="small muted">{t.description}</div>
                </td>
                <td>
                  <select className="select" aria-label={`How often: ${t.title}`} value={t.interval_days} onChange={(e) => save(t, t.enabled, Number(e.target.value))}>
                    {[1, 7, 14, 30].map((d) => <option key={d} value={d}>{EVERY[d]}</option>)}
                  </select>
                </td>
                <td className="small">
                  {t.last_run ? <span title={date(t.last_run, true)}>{relative(t.last_run)}</span> : <span className="dim">never</span>}
                  {t.enabled && t.next_due && <div className="dim">next: {until(t.next_due) === "now" ? "when the PC is quiet" : until(t.next_due)}</div>}
                </td>
                <td>
                  <Toggle on={t.enabled} label={`Run "${t.title}" on a schedule`} onChange={(v) => save(t, v, t.interval_days)} />
                </td>
                <td className="r">
                  {t.changes_system && confirm !== t.id ? (
                    <button className="btn sm" disabled={!!running} onClick={() => setConfirm(t.id)}>Run now</button>
                  ) : t.changes_system ? (
                    <span className="row" style={{ gap: 6, justifyContent: "flex-end" }}>
                      <button className="btn ghost sm" onClick={() => setConfirm(null)}>Cancel</button>
                      <button className="btn primary sm" onClick={() => runNow(t)}>Delete files</button>
                    </span>
                  ) : (
                    <button className="btn sm" disabled={!!running} onClick={() => runNow(t)}>
                      {running === t.id ? <Spinner /> : null} Run now
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <Panel className="mt-2" title="History" sub="The last 100 runs, including deferrals. Kept for a year." actions={<button className="btn ghost sm icon" onClick={data.reload} aria-label="Refresh"><I.refresh /></button>}>
        {history.length === 0 ? (
          <Empty title="Nothing has run yet">Scheduled tasks start two minutes after NOCTURN opens.</Empty>
        ) : (
          <div className="table-wrap" style={{ maxHeight: 420 }}>
            <table className="table">
              <thead><tr><th style={{ width: 140 }}>When</th><th style={{ width: 210 }}>Task</th><th style={{ width: 140 }}>Result</th><th>Details</th></tr></thead>
              <tbody>
                {history.map((h) => (
                  <tr key={h.id}>
                    <td className="small" title={date(h.started_at, true)}>{relative(h.started_at)}</td>
                    <td className="small">{title(h.task)}{h.trigger === "manual" && <span className="dim"> · by you</span>}</td>
                    <td><span className={`tag ${STATUS[h.status].cls}`}>{h.status === "ok" && <I.check />} {STATUS[h.status].label}</span></td>
                    <td className="small muted">{h.summary}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>

      <div className="mt-2">
        <Note>
          These run inside NOCTURN, so they only happen while it's open or minimised — closing the window stops them. Only "Clean temporary files" changes anything, and it's off until you turn it on. NOCTURN doesn't defragment SSDs, clean the registry or run third-party "boosters" — Windows already maintains drives itself.
        </Note>
      </div>
    </div>
  );
}
