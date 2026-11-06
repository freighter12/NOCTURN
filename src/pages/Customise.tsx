// Customise: taskbar, File Explorer and notification settings. Pick choices,
// preview exactly what changes, apply (journaled, per-user, no admin), undo
// from the toast or History. "Windows default" removes the value entirely.

import { useMemo, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Seg, Spinner } from "../components/ui";
import { api, errorMessage, type ApplyReport, type CustomPreviewItem, type CustomRequest, type CustomSection, type CustomSetting } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { takesLabel } from "../lib/format";

const SECTIONS: { value: CustomSection; label: string }[] = [
  { value: "taskbar", label: "Taskbar" },
  { value: "explorer", label: "File Explorer" },
  { value: "notifications", label: "Notifications" },
];
const sectionLabel = (s: CustomSection) => SECTIONS.find((x) => x.value === s)?.label ?? s;

function defaultLabel(s: CustomSetting) {
  const d = s.choices.find((c) => c.id === s.windows_default);
  return d ? `Windows default (${d.label.toLowerCase()})` : "Windows default";
}

type Review = { request: CustomRequest; label: string; items: CustomPreviewItem[] };

export function Customise() {
  const { dataVersion, bumpData, toast, navigate } = useApp();
  const settings = useLoad(() => api.customise(), [dataVersion]);
  const [section, setSection] = useState<CustomSection>("taskbar");
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [review, setReview] = useState<Review | null>(null);
  const [busy, setBusy] = useState<"preview" | "apply" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [needsExplorer, setNeedsExplorer] = useState(false);
  const [confirmRestart, setConfirmRestart] = useState(false);
  const [restarting, setRestarting] = useState(false);

  const rows = useMemo(() => (settings.data ?? []).filter((s) => s.section === section), [settings.data, section]);
  const pending = Object.entries(draft).filter(([id, choice]) => settings.data?.find((s) => s.id === id)?.current !== choice);
  const pendingIn = (sec: CustomSection) => pending.filter(([id]) => settings.data?.find((s) => s.id === id)?.section === sec).length;
  const customised = rows.filter((s) => s.current !== "default").length;

  function choose(s: CustomSetting, choice: string) {
    setReview(null);
    setError(null);
    setDraft((d) => {
      const n = { ...d };
      if (choice === s.current) delete n[s.id];
      else n[s.id] = choice;
      return n;
    });
  }

  async function openReview(request: CustomRequest, label: string) {
    setBusy("preview");
    setError(null);
    try {
      const items = await api.previewCustomise(request);
      setReview({ request, label, items });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  function report(r: ApplyReport, label: string) {
    const ok = r.status === "completed";
    const undoable = ok || r.status === "partial";
    const failed = r.results.filter((x) => x.status === "failed");
    toast({
      tone: ok || r.status === "nothing_to_do" ? "good" : "warning",
      title: ok ? `${label} — done` : r.status === "nothing_to_do" ? "Already set" : r.status === "partial" ? `${label} — partly applied` : `${label} — not applied`,
      detail: ok ? r.notes[0] ?? "Takes effect now. Recorded in History." : failed[0]?.error ?? r.notes[0],
      action: undoable
        ? {
            label: "Undo",
            run: () =>
              api
                .rollback(r.tx_id)
                .then(bumpData)
                .catch((e) => toast({ tone: "critical", title: "Undo failed", detail: errorMessage(e) })),
          }
        : undefined,
    });
  }

  async function apply() {
    if (!review) return;
    setBusy("apply");
    setError(null);
    try {
      const r = await api.applyCustomise(review.request, review.label);
      report(r, review.label);
      const changed = review.items.filter((i) => i.will_change);
      if (r.results.some((x) => x.status === "applied") && changed.some((i) => i.takes === "app_restart")) setNeedsExplorer(true);
      if ("selections" in review.request) setDraft({});
      setReview(null);
      bumpData();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function restartExplorer() {
    setRestarting(true);
    try {
      await api.restartExplorer();
      setNeedsExplorer(false);
      toast({ tone: "good", title: "File Explorer restarted", detail: "The taskbar and Explorer windows reloaded with your settings." });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't restart File Explorer", detail: errorMessage(e) });
    } finally {
      setRestarting(false);
      setConfirmRestart(false);
    }
  }

  const changes = review?.items.filter((i) => i.will_change) ?? [];

  return (
    <div className="page">
      <PageHead
        eyebrow="Customise"
        title="Taskbar, File Explorer and notifications"
        desc="Everyday Windows settings in one place. Each change is previewed, applies to your account only (no administrator approval), and can be undone from History. “Windows default” removes NOCTURN's value so Windows behaves as it does out of the box."
        actions={
          <button className="btn ghost" disabled={!!busy || !settings.data} onClick={() => openReview({ resetAll: true }, "Reset everything to Windows defaults")}>
            Reset all to Windows defaults
          </button>
        }
      />

      <section className="panel flush">
        <div className="row between" style={{ padding: "12px 16px", borderBottom: "1px solid var(--line)" }}>
          <Seg<CustomSection>
            value={section}
            onChange={(v) => {
              setSection(v);
              setReview(null);
            }}
            options={SECTIONS.map((s) => ({ value: s.value, label: pendingIn(s.value) ? `${s.label} (${pendingIn(s.value)})` : s.label }))}
          />
          <div className="row">
            <span className="small dim">{settings.data ? `${customised} of ${rows.length} changed from the default` : ""}</span>
            <button className="btn sm icon" onClick={settings.reload} aria-label="Refresh"><I.refresh /></button>
            <button
              className="btn sm"
              disabled={!!busy || !settings.data || customised === 0}
              onClick={() => openReview({ reset: section }, `Reset ${sectionLabel(section)} to Windows defaults`)}
            >
              Reset {sectionLabel(section)}
            </button>
          </div>
        </div>
        {settings.loading && !settings.data && <Loading label="Reading your current settings…" />}
        {settings.error && <div style={{ padding: 16 }}><ErrorNote message={settings.error} onRetry={settings.reload} /></div>}
        {settings.data && rows.length === 0 && <Empty title="Nothing here" />}
        {rows.length > 0 && (
          <table className="table">
            <tbody>
              {rows.map((s) => {
                const value = draft[s.id] ?? s.current ?? "__custom";
                const dirty = s.id in draft && draft[s.id] !== s.current;
                return (
                  <tr key={s.id} style={dirty ? { background: "var(--moon-wash)" } : undefined}>
                    <td style={{ width: "62%" }}>
                      <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                        <span style={{ fontWeight: 500 }}>{s.title}</span>
                        {!s.available && <span className="tag" title={`Needs Windows build ${s.min_build} or later`}>Not on this Windows version</span>}
                        {s.current === null && !s.error && <span className="tag warning" title="Set to a value NOCTURN doesn't offer, possibly by another tool">Custom value</span>}
                      </div>
                      <div className="small muted">{s.description}</div>
                      {s.error && <div className="small" style={{ color: "#ee8a8a" }}>{s.error}</div>}
                    </td>
                    <td className="small dim">{s.takes === "app_restart" ? "Reopen File Explorer" : takesLabel[s.takes]}</td>
                    <td className="r" style={{ width: 230 }}>
                      <select
                        className="select"
                        style={{ width: 220 }}
                        value={value}
                        disabled={!s.available || !!busy || !!s.error}
                        aria-label={s.title}
                        onChange={(e) => choose(s, e.target.value)}
                      >
                        {s.current === null && <option value="__custom" disabled>{s.current_label}</option>}
                        <option value="default">{defaultLabel(s)}</option>
                        {s.choices.map((c) => (
                          <option key={c.id} value={c.id}>{c.label}</option>
                        ))}
                      </select>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>

      {(pending.length > 0 || review || error) && (
        <section className="panel mt-2" style={{ borderColor: "var(--line-3)" }}>
          {error && <ErrorNote message={error} />}
          {!review && pending.length > 0 && (
            <div className="row between">
              <span className="small">{pending.length} setting{pending.length === 1 ? "" : "s"} changed — nothing is applied until you review it.</span>
              <div className="row">
                <button className="btn ghost sm" disabled={!!busy} onClick={() => setDraft({})}>Discard</button>
                <button
                  className="btn primary sm"
                  disabled={!!busy}
                  onClick={() =>
                    openReview(
                      { selections: pending.map(([id, choice]) => ({ id, choice })) },
                      pending.length === 1 ? settings.data?.find((s) => s.id === pending[0][0])?.title ?? "Customise" : `${pending.length} settings`,
                    )
                  }
                >
                  {busy === "preview" ? <Spinner /> : null} Review changes <I.arrow />
                </button>
              </div>
            </div>
          )}
          {review && (
            <div className="stack" style={{ gap: 10 }}>
              <div className="row between">
                <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>{review.label}</h3>
                <span className="small dim">{changes.length ? `${changes.length} change${changes.length === 1 ? "" : "s"}` : "Nothing to change"}</span>
              </div>
              {changes.length === 0 ? (
                <div className="small muted">Every setting here is already at the value you picked.</div>
              ) : (
                <table className="table">
                  <thead>
                    <tr>
                      <th>Setting</th>
                      <th>Now</th>
                      <th>After</th>
                      <th>Registry value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {changes.map((i) => (
                      <tr key={i.id}>
                        <td>{i.title}</td>
                        <td className="muted">{i.current}</td>
                        <td>{i.proposed}</td>
                        <td className="mono dim selectable" style={{ maxWidth: 260, overflowWrap: "anywhere" }}>{i.target}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              <div className="row between">
                <span className="small dim">Your account only · no administrator approval · undo from History</span>
                <div className="row">
                  <button className="btn ghost sm" disabled={busy === "apply"} onClick={() => setReview(null)}>Back</button>
                  <button className="btn primary sm" disabled={!!busy || changes.length === 0} onClick={apply}>
                    {busy === "apply" ? <Spinner /> : null} Apply {changes.length || ""}
                  </button>
                </div>
              </div>
            </div>
          )}
        </section>
      )}

      {needsExplorer && (
        <div className="mt-2">
          <Note tone="warning">
            <div className="row between" style={{ gap: 12 }}>
              <span>Some changes show up only after File Explorer reopens. Restarting it reloads the taskbar and closes any open Explorer windows; other apps keep running.</span>
              {confirmRestart ? (
                <span className="row">
                  <button className="btn ghost sm" disabled={restarting} onClick={() => setConfirmRestart(false)}>Cancel</button>
                  <button className="btn primary sm" disabled={restarting} onClick={restartExplorer}>
                    {restarting ? <Spinner /> : null} Restart now
                  </button>
                </span>
              ) : (
                <span className="row">
                  <button className="btn ghost sm" onClick={() => setNeedsExplorer(false)}>Later</button>
                  <button className="btn sm" onClick={() => setConfirmRestart(true)}>Restart File Explorer</button>
                </span>
              )}
            </div>
          </Note>
        </div>
      )}

      <div className="mt-2">
        <Note>
          These are per-user settings with documented values. Notification changes take effect after you sign out. For performance and privacy settings see{" "}
          <a href="#" onClick={(e) => { e.preventDefault(); navigate("tweak"); }}>NOCTURN Tweak</a>.
        </Note>
      </div>
    </div>
  );
}
