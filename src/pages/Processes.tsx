// Process Manager: live grouped process list with search and filters, session
// usage statistics, evidence-based "heavy" flags, per-app priority and CPU
// affinity, and saved rules that NOCTURN applies whenever the app starts.

import { useEffect, useMemo, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Seg, Spinner, Toggle, CountUp } from "../components/ui";
import { api, errorMessage, type Priority, type ProcDetails, type ProcGroup, type ProcOverview, type ProcessRule } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { maskBits, PRIORITIES, toMask } from "../lib/cpu";
import { bytes, duration, relative } from "../lib/format";

type Filter = "all" | "apps" | "background" | "flagged";
type Sort = "cpu" | "memory" | "disk" | "name";

const FLAG_LABEL: Record<string, string> = { high_cpu: "High CPU", high_memory: "High memory", disk_heavy: "Heavy disk" };
const FLAG_HINT: Record<string, string> = {
  high_cpu: "Using ≥15% of total CPU now and ≥8% on average since NOCTURN started",
  high_memory: "Using more than 10% of your RAM (and at least 1.5 GB)",
  disk_heavy: "Reading or writing more than 25 MB/s",
};

export function Processes() {
  const [data, setData] = useState<ProcOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("cpu");
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const rules = useLoad(() => api.processRules(), []);

  useEffect(() => {
    let alive = true;
    const load = () =>
      api
        .processOverview()
        .then((d) => alive && (setData(d), setError(null)))
        .catch((e) => alive && setError(errorMessage(e)));
    load();
    const t = setInterval(load, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const rows = useMemo(() => {
    const g = (data?.groups ?? [])
      .filter((p) => (filter === "apps" ? p.has_window : filter === "background" ? !p.has_window : filter === "flagged" ? p.flags.length > 0 : true))
      .filter((p) => !q || `${p.name} ${p.exe ?? ""}`.toLowerCase().includes(q.toLowerCase()));
    const by: Record<Sort, (a: ProcGroup, b: ProcGroup) => number> = {
      cpu: (a, b) => b.cpu - a.cpu,
      memory: (a, b) => b.memory - a.memory,
      disk: (a, b) => b.disk_bps - a.disk_bps,
      name: (a, b) => a.name.localeCompare(b.name),
    };
    return [...g].sort(by[sort]);
  }, [data, filter, q, sort]);

  const apps = data?.groups.filter((g) => g.has_window).length ?? 0;
  const flagged = data?.groups.filter((g) => g.flags.length).length ?? 0;
  const sel = data?.groups.find((g) => g.name === selected) ?? null;

  return (
    <div className="page">
      <PageHead
        eyebrow="Processes"
        title="Process manager"
        desc="Everything running on this PC, grouped by app. Change an app's priority or which CPUs it uses, and save rules that NOCTURN applies every time that app starts."
      />

      <div className="grid cols-4">
        <Stat label="Processes" value={data?.total_processes} foot={`${data?.groups.length ?? "—"} distinct programs`} />
        <Stat label="Apps with windows" value={data ? apps : undefined} foot={`${data ? data.groups.length - apps : "—"} running in the background`} />
        <Stat label="Flagged as heavy" value={data ? flagged : undefined} foot="Based on measured CPU, memory and disk use" />
        <Stat label="Saved rules" value={rules.data?.length} foot={`${rules.data?.filter((r) => r.enabled).length ?? "—"} active`} />
      </div>

      <div className="grid mt-2" style={{ gridTemplateColumns: sel ? "minmax(0, 1fr) 400px" : "minmax(0, 1fr)", alignItems: "start" }}>
        <section className="panel flush">
          <div className="row between wrap" style={{ padding: "12px 16px", borderBottom: "1px solid var(--line)", gap: 10 }}>
            <div className="row">
              <Seg<Filter> value={filter} onChange={setFilter} options={[{ value: "all", label: "All" }, { value: "apps", label: "Apps" }, { value: "background", label: "Background" }, { value: "flagged", label: `Heavy${flagged ? ` (${flagged})` : ""}` }]} />
              <div className="row" style={{ position: "relative" }}>
                <I.search style={{ position: "absolute", left: 9, width: 14, height: 14, color: "var(--ink-3)" }} />
                <input className="input" style={{ width: 220, height: 30, paddingLeft: 30 }} placeholder="Search name or path" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search processes" />
              </div>
            </div>
            <Seg<Sort> value={sort} onChange={setSort} options={[{ value: "cpu", label: "CPU" }, { value: "memory", label: "Memory" }, { value: "disk", label: "Disk" }, { value: "name", label: "Name" }]} />
          </div>
          {error && <div style={{ padding: 16 }}><ErrorNote message={error} /></div>}
          {!data && !error && <Loading label="Reading processes…" />}
          {data && rows.length === 0 && <Empty title="No matching processes" />}
          {rows.length > 0 && (
            <div className="table-wrap" style={{ maxHeight: "calc(100vh - 330px)" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th className="r">CPU</th>
                    <th className="r" title="Average since NOCTURN started">Avg CPU</th>
                    <th className="r">Memory</th>
                    <th className="r" title="Highest seen since NOCTURN started">Peak mem</th>
                    <th className="r">Disk</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((p) => (
                    <tr key={p.name} onClick={() => setSelected(p.name === selected ? null : p.name)} style={{ cursor: "default", background: p.name === selected ? "rgba(var(--overlay-rgb), .045)" : undefined }}>
                      <td style={{ maxWidth: 0, width: "44%" }}>
                        <div className="row" style={{ gap: 7, flexWrap: "wrap" }}>
                          <span className="status-dot" title={p.has_window ? "App with a window" : "Background process"} style={{ background: p.has_window ? "var(--moon-2)" : "var(--bg-4)" }} />
                          <span style={{ fontWeight: 500 }} className="truncate">{p.name}</span>
                          {p.instances > 1 && <span className="dim small">×{p.instances}</span>}
                          {p.flags.map((f) => (
                            <span key={f} className="tag warning" title={FLAG_HINT[f]}>{FLAG_LABEL[f]}</span>
                          ))}
                          {p.rule && <span className={`tag ${p.rule.enabled ? "good" : ""}`} title="Saved rule">Rule</span>}
                          {p.protected && <span className="tag" title="Core Windows process — NOCTURN won't change it"><I.lock /> Windows</span>}
                        </div>
                        {p.exe && <div className="mono dim truncate" title={p.exe}>{p.exe}</div>}
                      </td>
                      <td className="r num">{p.cpu.toFixed(1)}%</td>
                      <td className="r num muted">{p.cpu_avg.toFixed(1)}%</td>
                      <td className="r num">{bytes(p.memory)}</td>
                      <td className="r num muted">{bytes(p.mem_peak)}</td>
                      <td className="r num">{p.disk_bps ? `${bytes(p.disk_bps)}/s` : "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <div className="small dim" style={{ padding: "8px 16px" }}>
            Refreshes every 3 s. Averages and peaks cover the time since NOCTURN started. Click a row for priority, CPU and rule options.
          </div>
        </section>

        {sel && <DetailPanel key={sel.name} group={sel} logical={data?.logical_cpus ?? 0} onClose={() => setSelected(null)} onRulesChanged={rules.reload} />}
      </div>

      <RulesPanel rules={rules.data} error={rules.error} reload={rules.reload} />
    </div>
  );
}

function Stat({ label, value, foot }: { label: string; value: number | undefined; foot: string }) {
  return (
    <div className="panel stat">
      <div className="label">{label}</div>
      <div className="value num">{value === undefined ? "—" : <CountUp value={value} />}</div>
      <div className="foot">{foot}</div>
    </div>
  );
}

// -------------------------------------------------------------- details

function DetailPanel({ group, onClose, onRulesChanged }: { group: ProcGroup; logical: number; onClose: () => void; onRulesChanged: () => void }) {
  const { toast } = useApp();
  const det = useLoad<ProcDetails>(() => api.processDetails(group.name), [group.name]);
  const [priority, setPriority] = useState<Priority>("normal");
  const [cpus, setCpus] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmEnd, setConfirmEnd] = useState(false);
  const [ruleOn, setRuleOn] = useState(false);

  const systemBits = useMemo(() => [...maskBits(det.data?.system_mask)].sort((a, b) => a - b), [det.data?.system_mask]);

  // Seed the controls from the saved rule, else the first instance's state.
  useEffect(() => {
    const d = det.data;
    if (!d) return;
    const rule = d.rule;
    const first = d.instances[0];
    const fromLabel = PRIORITIES.find((p) => p.label === first?.priority)?.value;
    setPriority(rule?.priority ?? fromLabel ?? "normal");
    setCpus(maskBits(rule?.affinity ?? first?.affinity ?? d.system_mask));
    setRuleOn(!!rule?.enabled);
  }, [det.data]);

  const allCpus = systemBits.length > 0 && systemBits.every((b) => cpus.has(b));
  const mask = toMask(cpus);

  async function run(label: string, f: () => Promise<{ ok: boolean; error: string | null }[] | void>) {
    setBusy(label);
    try {
      const out = await f();
      if (Array.isArray(out)) {
        const ok = out.filter((o) => o.ok).length;
        const err = out.find((o) => !o.ok)?.error;
        toast({ tone: ok === out.length ? "good" : ok ? "warning" : "critical", title: `${label}: ${ok} of ${out.length} process(es)`, detail: err ?? undefined });
      }
      det.reload();
    } catch (e) {
      toast({ tone: "critical", title: `${label} failed`, detail: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  }

  async function saveRule(enabled: boolean) {
    await run("Rule saved", async () => {
      await api.saveProcessRule({ name: group.name, priority: priority === "normal" ? null : priority, affinity: allCpus ? null : mask, enabled, note: "" });
      setRuleOn(enabled);
      onRulesChanged();
      toast({ tone: "good", title: enabled ? `Rule saved for ${group.name}` : `Rule for ${group.name} turned off`, detail: enabled ? "Applied now to running instances, and from now on whenever it starts." : undefined });
    });
  }

  const ruleInvalid = priority === "normal" && allCpus;

  return (
    <Panel
      lit
      title={group.name}
      sub={`${group.instances} process${group.instances === 1 ? "" : "es"} · ${group.has_window ? "app" : "background"}${group.first_seen ? ` · seen ${relative(new Date(group.first_seen).toISOString())}` : ""}`}
      actions={<button className="btn ghost sm icon" onClick={onClose} aria-label="Close details"><I.x /></button>}
    >
      {det.loading && !det.data && <Loading label="Reading priority and CPU affinity…" />}
      {det.error && <ErrorNote message={det.error} onRetry={det.reload} />}
      {group.protected && <Note>This is a core Windows process. NOCTURN shows it for information but won't change or end it.</Note>}
      {det.data && !group.protected && (
        <div className="stack" style={{ gap: 16 }}>
          <dl className="kv">
            <dt>CPU now / avg / peak</dt>
            <dd className="num">{group.cpu.toFixed(1)}% / {group.cpu_avg.toFixed(1)}% / {group.cpu_peak.toFixed(1)}%</dd>
            <dt>Memory now / peak</dt>
            <dd className="num">{bytes(group.memory)} / {bytes(group.mem_peak)}</dd>
            <dt>Priority</dt>
            <dd>{[...new Set(det.data.instances.map((i) => i.priority ?? "no access"))].join(", ")}</dd>
            <dt>Running for</dt>
            <dd>{duration(Math.max(...det.data.instances.map((i) => i.run_time_secs)))}</dd>
          </dl>

          <div className="field">
            <label htmlFor="prio">Priority</label>
            <div className="row">
              <select id="prio" className="select" value={priority} onChange={(e) => setPriority(e.target.value as Priority)}>
                {PRIORITIES.map((p) => <option key={p.value} value={p.value}>{p.label}</option>)}
              </select>
              <button className="btn" disabled={!!busy} onClick={() => run("Priority set", () => api.setProcessPriority(group.name, priority))}>
                {busy === "Priority set" ? <Spinner /> : null} Apply
              </button>
            </div>
            <div className="hint">{PRIORITIES.find((p) => p.value === priority)?.hint} Realtime is never offered — it can freeze Windows.</div>
          </div>

          <div className="field">
            <label>CPUs this app may use</label>
            <div className="cpu-grid" role="group" aria-label="CPU affinity">
              {systemBits.map((b) => (
                <label key={b} className={`cpu-cell ${cpus.has(b) ? "on" : ""}`} title={`Logical CPU ${b}`}>
                  <input
                    type="checkbox"
                    checked={cpus.has(b)}
                    onChange={() =>
                      setCpus((c) => {
                        const n = new Set(c);
                        n.has(b) ? n.delete(b) : n.add(b);
                        return n;
                      })
                    }
                  />
                  {b}
                </label>
              ))}
            </div>
            <div className="row">
              <button className="btn ghost sm" onClick={() => setCpus(new Set(systemBits))}>All CPUs</button>
              <button className="btn ghost sm" onClick={() => setCpus(new Set(systemBits.filter((b) => b !== 0)))}>All except CPU 0</button>
              <span style={{ flex: 1 }} />
              <button className="btn" disabled={!!busy || cpus.size === 0} onClick={() => run("CPU affinity set", () => api.setProcessAffinity(group.name, mask))}>
                {busy === "CPU affinity set" ? <Spinner /> : null} Apply
              </button>
            </div>
            <div className="hint">Restricting CPUs rarely helps modern games and can hurt them — measure before keeping it. Changes last until the app closes unless saved as a rule.</div>
          </div>

          <hr className="divider" style={{ margin: 0 }} />

          <div className="list-item" style={{ alignItems: "center", border: 0, padding: 0 }}>
            <div className="body">
              <div className="title">Apply every time {group.name} starts</div>
              <div className="desc">
                Saves the priority{allCpus ? "" : " and CPU selection"} above as a rule. NOCTURN applies it within a few seconds of the app starting, while NOCTURN is running.
              </div>
            </div>
            <Toggle on={ruleOn} disabled={!!busy || (!ruleOn && ruleInvalid)} onChange={(v) => saveRule(v)} label={`Rule for ${group.name}`} />
          </div>
          {ruleInvalid && !ruleOn && <div className="small dim">Pick a non-Normal priority or restrict CPUs to save a rule.</div>}
          {det.data.rule?.last_error && <Note tone="warning">Last time the rule was applied: {det.data.rule.last_error}</Note>}

          <hr className="divider" style={{ margin: 0 }} />

          <div className="row between">
            <span className="small muted">Unsaved work in {group.name} is lost if you end it.</span>
            {confirmEnd ? (
              <span className="row" style={{ gap: 6 }}>
                <button className="btn ghost sm" onClick={() => setConfirmEnd(false)}>Keep</button>
                <button
                  className="btn danger sm"
                  onClick={() =>
                    run("End app", async () => {
                      const [r] = await api.closeApps([group.name]);
                      setConfirmEnd(false);
                      toast({ tone: r.ended && !r.failed ? "good" : "warning", title: r.ended ? `Ended ${r.ended} process(es)` : `Couldn't end ${group.name}`, detail: r.error ?? undefined });
                      onClose();
                    })
                  }
                >
                  End {group.instances > 1 ? "all" : ""}
                </button>
              </span>
            ) : (
              <button className="btn ghost sm" onClick={() => setConfirmEnd(true)}>End…</button>
            )}
          </div>
        </div>
      )}
    </Panel>
  );
}

// ----------------------------------------------------------------- rules

function RulesPanel({ rules, error, reload }: { rules: ProcessRule[] | null; error: string | null; reload: () => void }) {
  const { toast } = useApp();
  async function toggle(r: ProcessRule, enabled: boolean) {
    try {
      await api.saveProcessRule({ name: r.name, priority: r.priority, affinity: r.affinity, enabled, note: r.note });
      reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't update rule", detail: errorMessage(e) });
    }
  }
  async function remove(r: ProcessRule) {
    try {
      await api.deleteProcessRule(r.name);
      reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't delete rule", detail: errorMessage(e) });
    }
  }
  return (
    <Panel className="mt-2" title="Saved rules" sub="Applied automatically when the app starts, while NOCTURN is running. Every application is recorded in the audit log.">
      {error && <ErrorNote message={error} onRetry={reload} />}
      {rules && rules.length === 0 && <Empty title="No rules yet">Select an app above and turn on “Apply every time it starts”.</Empty>}
      {rules && rules.length > 0 && (
        <table className="table">
          <thead>
            <tr>
              <th>App</th>
              <th>Priority</th>
              <th>CPUs</th>
              <th className="r">Applied</th>
              <th className="r" style={{ width: 160 }} />
            </tr>
          </thead>
          <tbody>
            {rules.map((r) => (
              <tr key={r.name}>
                <td>
                  <div style={{ fontWeight: 500 }}>{r.name}</div>
                  {r.last_error && <div className="small" style={{ color: "var(--ink-warning)" }}>{r.last_error}</div>}
                </td>
                <td className="small">{PRIORITIES.find((p) => p.value === r.priority)?.label ?? "unchanged"}</td>
                <td className="small mono">{r.affinity ? `${maskBits(r.affinity).size} CPU(s) · ${r.affinity}` : "unchanged"}</td>
                <td className="r small muted">{r.applied_count}× {r.last_applied_at ? `· ${relative(r.last_applied_at)}` : ""}</td>
                <td className="r">
                  <span className="row" style={{ justifyContent: "flex-end", gap: 8 }}>
                    <Toggle on={r.enabled} onChange={(v) => toggle(r, v)} label={`Enable rule for ${r.name}`} />
                    <button className="btn ghost sm" onClick={() => remove(r)}>Delete</button>
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  );
}
