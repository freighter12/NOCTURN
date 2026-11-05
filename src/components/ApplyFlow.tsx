// The one path every system change takes: preview → explicit approval →
// progress → verified per-change report with rollback available.
// Used by TWEAK pages, profiles, dashboard recommendations and AI plans.

import { useEffect, useMemo, useState } from "react";
import { api, errorMessage, type ApplyReport, type Preview, type Progress, type StartupItem, type TweakView } from "../lib/api";
import { useApp } from "../lib/app";
import { takesLabel } from "../lib/format";
import { I } from "./Icons";
import { ErrorNote, Note, RiskTag, Spinner, StatusTag } from "./ui";

type Phase = "loading" | "review" | "running" | "done" | "error";

export function ApplyFlow() {
  const { applyIntent: intent, closeApply, settings, toast, bumpData, navigate } = useApp();
  const [phase, setPhase] = useState<Phase>("loading");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [tweaks, setTweaks] = useState<TweakView[]>([]);
  const [startup, setStartup] = useState<StartupItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [restorePoint, setRestorePoint] = useState(true);
  const [ackAdvanced, setAckAdvanced] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [reports, setReports] = useState<ApplyReport[]>([]);

  useEffect(() => {
    if (!intent) return;
    setPhase("loading");
    setError(null);
    setReports([]);
    setProgress(null);
    setAckAdvanced(false);
    const jobs: Promise<unknown>[] = [api.tweaks().then(setTweaks)];
    if (intent.tweakIds.length) jobs.push(api.preview(intent.tweakIds).then(setPreview));
    else setPreview(null);
    if (intent.startupDisableIds?.length) jobs.push(api.startup().then(setStartup));
    Promise.all(jobs)
      .then(() => setPhase("review"))
      .catch((e) => {
        setError(errorMessage(e));
        setPhase("error");
      });
  }, [intent]);

  useEffect(() => {
    if (preview) setRestorePoint(preview.requires_admin && (settings?.restore_point_default ?? true));
  }, [preview, settings?.restore_point_default]);

  const byId = useMemo(() => new Map(tweaks.map((t) => [t.id, t])), [tweaks]);
  const startupRows = useMemo(
    () => (intent?.startupDisableIds ?? []).map((id) => startup.find((s) => s.id === id)).filter((s): s is StartupItem => !!s),
    [intent, startup],
  );
  const advanced = (intent?.tweakIds ?? []).filter((id) => byId.get(id)?.risk === "advanced");
  const needAck = advanced.length > 0 && (settings?.confirm_advanced ?? true);
  const changeCount = (preview?.changes ?? 0) + startupRows.filter((s) => s.enabled).length;
  const needsAdmin = (preview?.requires_admin ?? false) || startupRows.some((s) => s.requires_admin && s.enabled);

  if (!intent) return null;

  function finish(approved: boolean) {
    const txIds = reports.map((r) => r.tx_id);
    const results = reports.flatMap((r) => r.results);
    const ok = results.filter((r) => r.status === "applied").length;
    const failed = results.filter((r) => r.status === "failed" || r.status === "skipped");
    const summary = approved
      ? `Applied ${ok} change(s). ${failed.length ? `${failed.length} not applied: ${failed.map((f) => `${f.target} (${f.error ?? f.status})`).join("; ")}.` : "All changes verified."} ${reports.flatMap((r) => r.notes).join(" ")}`.trim()
      : "The user declined the plan. Nothing was changed.";
    intent?.onDone?.({ approved, txIds, summary });
    closeApply();
  }

  async function run() {
    if (!intent) return;
    setPhase("running");
    const out: ApplyReport[] = [];
    try {
      if (intent.tweakIds.length) {
        out.push(
          await api.apply(
            { tweak_ids: intent.tweakIds, source: intent.source, label: intent.label, create_restore_point: needsAdmin && restorePoint },
            setProgress,
          ),
        );
      }
      const toDisable = startupRows.filter((s) => s.enabled).map((s) => s.id);
      if (toDisable.length) {
        out.push(await api.setStartup(toDisable, false, intent.source === "ai" ? "ai" : "user", setProgress));
      }
      setReports(out);
      setPhase("done");
      bumpData();
      const results = out.flatMap((r) => r.results);
      const failed = results.filter((r) => r.status === "failed").length;
      toast({
        tone: failed ? "warning" : "good",
        title: failed ? "Some changes were not applied" : "Changes applied and verified",
        detail: `${results.filter((r) => r.status === "applied").length} applied · ${failed} failed`,
      });
    } catch (e) {
      setReports(out);
      setError(errorMessage(e));
      setPhase("error");
    }
  }

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && phase !== "running" && (phase === "done" ? finish(true) : finish(false))}>
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="apply-title">
        <div className="modal-head">
          <h2 id="apply-title">{phase === "done" ? "Results" : intent.title ?? "Review changes"}</h2>
          <p>
            {phase === "done"
              ? "Each change was read back from Windows to confirm it took effect."
              : intent.description ?? "Nothing changes until you approve. Every reversible change is recorded and can be rolled back from History."}
          </p>
        </div>

        <div className="modal-body">
          {phase === "loading" && (
            <div className="row" style={{ padding: 20 }}>
              <Spinner /> <span className="muted">Reading current values…</span>
            </div>
          )}
          {phase === "error" && error && <ErrorNote message={error} />}

          {phase === "review" && (
            <div className="stack" style={{ gap: 16 }}>
              {preview && groupByTweak(preview).map(([id, items]) => {
                const t = byId.get(id);
                return (
                  <div key={id}>
                    <div className="row between">
                      <div className="row" style={{ gap: 8 }}>
                        <b style={{ fontWeight: 600 }}>{items[0].tweak_title}</b>
                        {t && <RiskTag risk={t.risk} />}
                        {items.some((i) => i.requires_admin) && (
                          <span className="tag" title="Needs administrator approval">
                            <I.lock /> Admin
                          </span>
                        )}
                        {t && !t.reversible && <span className="tag warning">One-time</span>}
                      </div>
                      {t && <span className="dim small">{takesLabel[t.takes]}</span>}
                    </div>
                    {t?.caveat && <div className="small muted mt-1">{t.caveat}</div>}
                    <div className="mt-1">
                      {items.map((i, k) => (
                        <div key={k} className={`change-row ${i.will_change ? "" : "same"}`}>
                          <div className="target selectable">{i.target}</div>
                          <div className="val">{i.current}</div>
                          <div className="arrow">→</div>
                          <div className="val" style={{ color: i.will_change ? "var(--ink-0)" : undefined }}>
                            {i.will_change ? i.proposed : "already set"}
                          </div>
                          {i.note && <div className="small dim" style={{ gridColumn: "1 / -1" }}>{i.note}</div>}
                        </div>
                      ))}
                    </div>
                  </div>
                );
              })}

              {startupRows.length > 0 && (
                <div>
                  <b style={{ fontWeight: 600 }}>Startup apps to disable</b>
                  <div className="small muted mt-1">Entries are switched off the same way Task Manager does it — nothing is deleted, and you can re-enable them any time.</div>
                  <div className="mt-1">
                    {startupRows.map((s) => (
                      <div key={s.id} className={`change-row ${s.enabled ? "" : "same"}`}>
                        <div>
                          {s.name} <span className="target">{s.location_label}</span>
                        </div>
                        <div className="val">{s.enabled ? "Enabled" : "Disabled"}</div>
                        <div className="arrow">→</div>
                        <div className="val">{s.enabled ? "Disabled" : "already off"}</div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {preview && preview.irreversible.length > 0 && (
                <Note tone="warning">
                  {preview.irreversible.map((id) => byId.get(id)?.title ?? id).join(", ")} can't be undone (one-time maintenance actions).
                </Note>
              )}
              {needsAdmin && !preview?.is_elevated && (
                <Note>
                  Some changes affect all users, so Windows will ask for administrator approval once. NOCTURN only requests elevation for those specific changes.
                </Note>
              )}
              {changeCount === 0 && <Note tone="good">Everything here is already configured. There's nothing to change.</Note>}
            </div>
          )}

          {phase === "running" && (
            <div className="stack" style={{ padding: "18px 0", gap: 12 }}>
              <div className="row between">
                <span>{progress?.message ?? "Preparing…"}</span>
                <span className="dim num">{progress ? `${progress.done}/${progress.total}` : ""}</span>
              </div>
              <div className={`progress ${progress && progress.total ? "" : "indeterminate"}`}>
                <span style={{ width: progress && progress.total ? `${(progress.done / progress.total) * 100}%` : "30%" }} />
              </div>
              <div className="small dim">Recording original values first, then applying and verifying each change.</div>
            </div>
          )}

          {(phase === "done" || (phase === "error" && reports.length > 0)) && (
            <div className="stack" style={{ gap: 14 }}>
              {reports.map((r) => (
                <div key={r.tx_id} className="stack" style={{ gap: 8 }}>
                  <div className="row between">
                    <StatusTag status={r.status} />
                    <span className="dim small mono">{r.tx_id.slice(0, 8)}</span>
                  </div>
                  {r.restore_point && (
                    <Note tone={r.restore_point.ok ? "good" : "warning"}>
                      Restore point: {r.restore_point.ok ? "created" : "not created"} — {r.restore_point.message}
                    </Note>
                  )}
                  <div>
                    {r.results.map((c) => (
                      <div key={c.change_id} className="change-row">
                        <div>
                          <div className="target selectable">{c.target}</div>
                          {c.error && <div className="small" style={{ color: "#ee8a8a" }}>{c.error}</div>}
                        </div>
                        <div className="val">{c.before}</div>
                        <div className="arrow">→</div>
                        <div className="row" style={{ justifyContent: "flex-end", gap: 8 }}>
                          <span className="val">{c.after}</span>
                          <StatusTag status={c.status} />
                        </div>
                      </div>
                    ))}
                  </div>
                  {r.notes.map((n, i) => (
                    <div key={i} className="small muted">
                      {n}
                    </div>
                  ))}
                </div>
              ))}
            </div>
          )}
        </div>

        <div className="modal-foot">
          {phase === "review" && (
            <>
              <div className="left">
                {needsAdmin && (
                  <label className="row" style={{ gap: 8 }}>
                    <input type="checkbox" className="check" checked={restorePoint} onChange={(e) => setRestorePoint(e.target.checked)} />
                    Create a Windows restore point first
                  </label>
                )}
                {needAck && (
                  <label className="row" style={{ gap: 8 }}>
                    <input type="checkbox" className="check" checked={ackAdvanced} onChange={(e) => setAckAdvanced(e.target.checked)} />
                    I understand {advanced.length === 1 ? "the Advanced change" : `${advanced.length} Advanced changes`} may need measuring
                  </label>
                )}
              </div>
              <button className="btn ghost" onClick={() => finish(false)}>
                Cancel
              </button>
              <button className="btn primary" disabled={changeCount === 0 || (needAck && !ackAdvanced)} onClick={run}>
                Apply {changeCount} change{changeCount === 1 ? "" : "s"}
              </button>
            </>
          )}
          {phase === "running" && <span className="muted small">Please keep NOCTURN open…</span>}
          {(phase === "done" || phase === "error") && (
            <>
              {phase === "done" && (
                <button
                  className="btn ghost"
                  onClick={() => {
                    finish(true);
                    navigate("history");
                  }}
                >
                  <I.history /> View in History
                </button>
              )}
              <button className="btn" onClick={() => finish(phase === "done" || reports.length > 0)}>
                Close
              </button>
            </>
          )}
          {phase === "loading" && (
            <button className="btn ghost" onClick={() => finish(false)}>
              Cancel
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function groupByTweak(p: Preview): [string, Preview["items"]][] {
  const m = new Map<string, Preview["items"]>();
  for (const i of p.items) {
    if (!m.has(i.tweak_id)) m.set(i.tweak_id, []);
    m.get(i.tweak_id)!.push(i);
  }
  return [...m.entries()];
}

