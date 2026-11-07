// Services: every Windows service, sorted into tiers that decide what NOCTURN
// may change. Only start types change; each change is previewed, needs
// administrator approval, is journaled and can be undone from History.

import { Fragment, useMemo, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Seg, Spinner, StatusTag, CountUp } from "../components/ui";
import { api, errorMessage, type ServicePreviewItem, type ServiceTier, type ServiceView, type StartType } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { relative } from "../lib/format";

type Filter = "changeable" | ServiceTier | "all";

const TIER: Record<ServiceTier, { label: string; tone: string; title: string }> = {
  protected: { label: "Protected", tone: "solid", title: "Security, updates or core Windows — never changed" },
  windows: { label: "Windows", tone: "", title: "Part of Windows; not on NOCTURN's reviewed list, so read-only" },
  optional: { label: "Optional feature", tone: "good", title: "A Windows feature many people don't use; the note says what stops working" },
  third_party: { label: "Third-party", tone: "warning", title: "Installed by an app or driver" },
};

const START_LABEL: Record<StartType, string> = {
  automatic: "Automatic",
  automatic_delayed: "Automatic (delayed)",
  manual: "Manual",
  disabled: "Disabled",
};

export function Services() {
  const { dataVersion, bumpData, toast, navigate } = useApp();
  const services = useLoad(() => api.serviceList(), [dataVersion]);
  const changes = useLoad(() => api.serviceChanges(), [dataVersion]);
  const [filter, setFilter] = useState<Filter>("changeable");
  const [q, setQ] = useState("");
  const [runningOnly, setRunningOnly] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const [draft, setDraft] = useState<Record<string, StartType>>({});
  const [review, setReview] = useState<ServicePreviewItem[] | null>(null);
  const [busy, setBusy] = useState<"preview" | "apply" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const all = services.data ?? [];
  const count = (t: ServiceTier) => all.filter((s) => s.tier === t).length;
  const rows = useMemo(
    () =>
      all
        .filter((s) => (filter === "all" ? true : filter === "changeable" ? s.allowed.length > 0 : s.tier === filter))
        .filter((s) => !runningOnly || s.state === "running")
        .filter((s) => !q || `${s.name} ${s.display_name} ${s.publisher ?? ""}`.toLowerCase().includes(q.toLowerCase())),
    [all, filter, runningOnly, q],
  );
  const pending = Object.entries(draft).filter(([n, st]) => all.find((s) => s.name === n)?.start_kind !== st);

  function choose(s: ServiceView, st: StartType) {
    setReview(null);
    setError(null);
    setDraft((d) => {
      const n = { ...d };
      if (st === s.start_kind) delete n[s.name];
      else n[s.name] = st;
      return n;
    });
  }

  async function openReview() {
    setBusy("preview");
    setError(null);
    try {
      setReview(await api.previewServices(pending.map(([name, start]) => ({ name, start }))));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function apply() {
    if (!review) return;
    setBusy("apply");
    setError(null);
    try {
      const sel = review.filter((i) => i.will_change && !i.refused).map((i) => ({ name: i.name, start: draft[i.name] }));
      const r = await api.applyServices(sel);
      const ok = r.status === "completed";
      const undoable = ok || r.status === "partial";
      const err = r.results.find((x) => x.error)?.error;
      toast({
        tone: ok ? "good" : "warning",
        title: ok ? (sel.length === 1 ? `${review.find((i) => i.name === sel[0].name)?.display_name} changed` : `${sel.length} services changed`) : r.status === "partial" ? "Some services changed" : "Services not changed",
        detail: ok ? r.notes[0] ?? "Takes effect the next time Windows starts." : err ?? r.notes[0],
        action: undoable
          ? { label: "Undo", run: () => api.rollback(r.tx_id).then(bumpData).catch((e) => toast({ tone: "critical", title: "Undo failed", detail: errorMessage(e) })) }
          : undefined,
      });
      setDraft({});
      setReview(null);
      bumpData();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  const toApply = review?.filter((i) => i.will_change && !i.refused) ?? [];

  return (
    <div className="page">
      <PageHead
        eyebrow="Services"
        title="Windows services"
        desc="Programs Windows runs in the background. NOCTURN only changes how a service starts — never stops, deletes or reconfigures it — and only for optional Windows features and services installed by apps. Security, update and core services are protected."
        actions={
          <button className="btn" onClick={() => navigate("assistant", { prompt: "Look at the third-party and optional services on my PC. Which, if any, are worth setting to Manual, and what would I lose? Don't suggest disabling services as a general speed-up." })}>
            <I.assistant /> Ask AI
          </button>
        }
      />

      <div className="grid cols-4">
        <div className="panel stat">
          <div className="label">Services</div>
          <div className="value num">{services.data ? <CountUp value={all.length} /> : "—"}</div>
          <div className="foot">{services.data ? `${all.filter((s) => s.state === "running").length} running now` : ""}</div>
        </div>
        <div className="panel stat">
          <div className="label">Optional Windows features</div>
          <div className="value num">{services.data ? <CountUp value={count("optional")} /> : "—"}</div>
          <div className="foot">Reviewed, with what you'd lose</div>
        </div>
        <div className="panel stat">
          <div className="label">Third-party</div>
          <div className="value num">{services.data ? <CountUp value={count("third_party")} /> : "—"}</div>
          <div className="foot">Installed by apps and drivers</div>
        </div>
        <div className="panel stat">
          <div className="label">Protected</div>
          <div className="value num">{services.data ? <CountUp value={count("protected")} /> : "—"}</div>
          <div className="foot">{services.data ? `plus ${count("windows")} other Windows services, read-only` : ""}</div>
        </div>
      </div>

      <section className="panel flush mt-2">
        <div className="row between" style={{ padding: "12px 16px", borderBottom: "1px solid var(--line)", flexWrap: "wrap", gap: 10 }}>
          <div className="row" style={{ flexWrap: "wrap" }}>
            <Seg<Filter>
              value={filter}
              onChange={setFilter}
              options={[
                { value: "changeable", label: "Changeable" },
                { value: "optional", label: "Optional" },
                { value: "third_party", label: "Third-party" },
                { value: "windows", label: "Windows" },
                { value: "protected", label: "Protected" },
                { value: "all", label: "All" },
              ]}
            />
            <input className="input" style={{ width: 200, height: 30 }} placeholder="Search name or publisher" value={q} onChange={(e) => setQ(e.target.value)} />
            <label className="row small" style={{ gap: 6 }}>
              <input type="checkbox" className="check" checked={runningOnly} onChange={(e) => setRunningOnly(e.target.checked)} /> Running only
            </label>
          </div>
          <button className="btn sm icon" onClick={services.reload} aria-label="Refresh"><I.refresh /></button>
        </div>
        {services.loading && !services.data && <Loading label="Reading services, publishers and dependencies…" />}
        {services.error && <div style={{ padding: 16 }}><ErrorNote message={services.error} onRetry={services.reload} /></div>}
        {services.data && rows.length === 0 && <Empty title="No matching services" />}
        {rows.length > 0 && (
          <div className="table-wrap" style={{ maxHeight: "calc(100vh - 360px)" }}>
            <table className="table">
              <thead>
                <tr>
                  <th>Service</th>
                  <th style={{ width: 90 }}>State</th>
                  <th className="r" style={{ width: 220 }}>Start type</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((s) => {
                  const dirty = s.name in draft && draft[s.name] !== s.start_kind;
                  const isOpen = open === s.name;
                  return (
                    <Fragment key={s.name}>
                      <tr style={dirty ? { background: "var(--moon-wash)" } : undefined}>
                        <td style={{ maxWidth: 0 }}>
                          <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                            <button className="btn ghost sm" style={{ padding: "0 4px", height: 20 }} aria-label={isOpen ? "Hide details" : "Show details"} onClick={() => setOpen(isOpen ? null : s.name)}>
                              <span style={{ display: "inline-block", transform: isOpen ? "rotate(90deg)" : undefined }}><I.arrow /></span>
                            </button>
                            <span style={{ fontWeight: 500 }}>{s.display_name}</span>
                            <span className={`tag ${TIER[s.tier].tone}`} title={TIER[s.tier].title}>
                              {s.tier === "protected" && <I.lock />} {TIER[s.tier].label}
                            </span>
                            {s.anti_cheat && <span className="tag warning" title="Games that use this anti-cheat need it">Anti-cheat</span>}
                          </div>
                          <div className="small muted truncate" title={s.description ?? undefined}>
                            {[s.publisher, s.description].filter(Boolean).join(" · ") || s.name}
                          </div>
                        </td>
                        <td className="small">
                          <span style={{ color: s.state === "running" ? "var(--ink-good)" : "var(--ink-2)" }}>{s.state}</span>
                        </td>
                        <td className="r">
                          {s.allowed.length > 0 ? (
                            <select className="select" style={{ width: 200 }} aria-label={`Start type for ${s.display_name}`} value={draft[s.name] ?? s.start_kind ?? ""} disabled={!!busy} onChange={(e) => choose(s, e.target.value as StartType)}>
                              {!s.start_kind && <option value="" disabled>{s.start_label}</option>}
                              {s.allowed.map((a) => (
                                <option key={a} value={a}>{START_LABEL[a]}</option>
                              ))}
                            </select>
                          ) : (
                            <span className="small dim" title={s.tier_reason}>{s.start_label}</span>
                          )}
                        </td>
                      </tr>
                      {isOpen && (
                        <tr>
                          <td colSpan={3} style={{ background: "rgba(var(--overlay-rgb), 0.015)" }}>
                            <div className="stack" style={{ gap: 8, padding: "4px 0 4px 30px" }}>
                              {s.note && <div className="small" style={{ color: "var(--ink-warning)" }}>{s.note}</div>}
                              {s.description && <div className="small muted">{s.description}</div>}
                              <dl className="kv">
                                <dt>Service name</dt>
                                <dd className="mono selectable">{s.name}</dd>
                                <dt>Tier</dt>
                                <dd>{s.tier_reason}</dd>
                                <dt>Program</dt>
                                <dd className="mono selectable">{s.image ?? "—"}</dd>
                                <dt>Publisher</dt>
                                <dd>{s.publisher ?? "not stated by the file"}</dd>
                                <dt>Runs as</dt>
                                <dd>{s.account ?? "—"}</dd>
                                <dt>Needs</dt>
                                <dd>{s.depends_on.length ? s.depends_on.join(", ") : "nothing"}</dd>
                                <dt>Needed by</dt>
                                <dd>{s.dependents.length ? s.dependents.join(", ") : "no other service"}</dd>
                              </dl>
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {(pending.length > 0 || review || error) && (
        <section className="panel mt-2" style={{ borderColor: "var(--line-3)" }}>
          {error && <ErrorNote message={error} />}
          {!review && pending.length > 0 && (
            <div className="row between">
              <span className="small">{pending.length} service{pending.length === 1 ? "" : "s"} changed — nothing is applied until you review it.</span>
              <div className="row">
                <button className="btn ghost sm" disabled={!!busy} onClick={() => setDraft({})}>Discard</button>
                <button className="btn primary sm" disabled={!!busy} onClick={openReview}>
                  {busy === "preview" ? <Spinner /> : null} Review changes <I.arrow />
                </button>
              </div>
            </div>
          )}
          {review && (
            <div className="stack" style={{ gap: 10 }}>
              <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>Review service changes</h3>
              <table className="table">
                <thead>
                  <tr>
                    <th>Service</th>
                    <th>Now</th>
                    <th>After</th>
                    <th>Notes</th>
                  </tr>
                </thead>
                <tbody>
                  {review.map((i) => (
                    <tr key={i.name}>
                      <td>{i.display_name}</td>
                      <td className="muted">{i.current}</td>
                      <td>{i.refused ? <span className="dim">not changed</span> : i.proposed}</td>
                      <td className="small">
                        {i.refused && <div style={{ color: "var(--ink-critical)" }}>{i.refused}</div>}
                        {i.warnings.map((w) => (
                          <div key={w} style={{ color: "var(--ink-warning)" }}>{w}</div>
                        ))}
                        {!i.refused && !i.warnings.length && <span className="dim">—</span>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="row between">
                <span className="small dim">Needs administrator approval · takes effect at next start · undo from History</span>
                <div className="row">
                  <button className="btn ghost sm" disabled={busy === "apply"} onClick={() => setReview(null)}>Back</button>
                  <button className="btn primary sm" disabled={!!busy || toApply.length === 0} onClick={apply}>
                    {busy === "apply" ? <Spinner /> : <I.lock />} Apply {toApply.length || ""}
                  </button>
                </div>
              </div>
            </div>
          )}
        </section>
      )}

      <Panel className="mt-2" title="Changes made by NOCTURN" sub="Every service change is journalled with the original start type. Undo any of them from History." actions={<button className="btn ghost sm" onClick={() => navigate("history")}>Open History <I.arrow /></button>}>
        {changes.error && <ErrorNote message={changes.error} onRetry={changes.reload} />}
        {changes.data && changes.data.length === 0 && <Empty title="No service changes yet" />}
        {changes.data && changes.data.length > 0 && (
          <div className="list">
            {changes.data.slice(0, 8).map((c, i) => (
              <div key={i} className="list-item" style={{ alignItems: "center" }}>
                <div className="body">
                  <div className="title">{c.target}</div>
                  <div className="desc">
                    {relative(c.applied_at)} · {c.source === "ai" ? "approved from an AI plan" : "by you"}
                    {c.rolled_back_at ? ` · undone ${relative(c.rolled_back_at)}` : ""}
                  </div>
                </div>
                <StatusTag status={c.status} />
              </div>
            ))}
          </div>
        )}
      </Panel>
      <div className="mt-2">
        <Note>
          Manual means a service starts only when something needs it, which is the safe choice for most optional services. Disabled means it can't start at all; NOCTURN refuses to disable a service that another automatically starting service depends on. Microsoft Defender, Windows Update, the firewall and other security and core services can never be changed here.
        </Note>
      </div>
    </div>
  );
}
