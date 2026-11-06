// Benchmarks: CPU, memory, storage and boot time, stored so any run can be
// compared with an earlier one (e.g. before and after applying a profile).

import { useEffect, useMemo, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Note, PageHead, Panel } from "../components/ui";
import { api, errorMessage, type BenchMetric, type BenchProgress, type BenchRun, type BenchSuite } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { bytes, date, relative } from "../lib/format";

const SUITES: { id: BenchSuite; name: string; desc: string; time: string }[] = [
  { id: "cpu", name: "CPU", desc: "Integer and floating-point throughput on one core and on all cores.", time: "about 15 s" },
  { id: "memory", name: "Memory", desc: "Read, write and copy bandwidth, and random-access latency.", time: "about 8 s" },
  { id: "storage", name: "Storage", desc: "Sequential and 4K random speed with Windows' cache bypassed. Writes a temporary 1 GB file, then deletes it.", time: "about 25 s" },
  { id: "boot", name: "Boot time", desc: "Firmware time and Windows start-to-sign-in, from Windows' own records. Nothing is run.", time: "instant" },
];
const NAME: Record<BenchSuite, string> = { cpu: "CPU", memory: "Memory", storage: "Storage", boot: "Boot time" };
/** Changes smaller than this are normal run-to-run variation. */
const NOISE = 5;

export function fmt(v: number, unit: string) {
  const n = Math.abs(v) >= 100 ? v.toLocaleString(undefined, { maximumFractionDigits: 0 }) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(2);
  return `${n} ${unit}`;
}

function change(m: BenchMetric, base: BenchMetric | undefined) {
  if (!base || base.value === 0) return null;
  const pct = ((m.value - base.value) / base.value) * 100;
  const better = m.higher_is_better ? pct > 0 : pct < 0;
  const verdict = Math.abs(pct) < NOISE ? "same" : better ? "better" : "worse";
  return { pct, verdict };
}

function Compare({ run, base }: { run: BenchRun; base: BenchRun | null }) {
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Measurement</th>
          {base && <th className="r">Baseline</th>}
          <th className="r">This run</th>
          {base && <th className="r" style={{ width: 170 }}>Change</th>}
        </tr>
      </thead>
      <tbody>
        {run.metrics.map((m) => {
          const b = base?.metrics.find((x) => x.key === m.key);
          const c = change(m, b);
          return (
            <tr key={m.key}>
              <td>
                {m.label} <span className="dim small">{m.higher_is_better ? "(higher is better)" : "(lower is better)"}</span>
              </td>
              {base && <td className="r num muted">{b ? fmt(b.value, b.unit) : "—"}</td>}
              <td className="r num" style={{ fontWeight: 500 }}>{fmt(m.value, m.unit)}</td>
              {base && (
                <td className="r">
                  {c ? (
                    <span className={`tag ${c.verdict === "better" ? "good" : c.verdict === "worse" ? "warning" : ""}`} title={c.verdict === "same" ? `Within ±${NOISE}%, normal variation` : undefined}>
                      {c.verdict === "better" ? <I.check /> : null}
                      {c.pct > 0 ? "+" : ""}
                      {c.pct.toFixed(1)}% · {c.verdict === "same" ? "no real change" : c.verdict}
                    </span>
                  ) : (
                    <span className="dim">—</span>
                  )}
                </td>
              )}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

export function Benchmarks() {
  const { toast, navigate } = useApp();
  const runs = useLoad(() => api.benchmarks(), []);
  // Drives only; the storage page also lists folders like the user profile.
  const roots = useLoad(() => api.storageRoots().then((r) => r.filter((x) => /^[A-Za-z]:\\?$/.test(x.path))), []);
  const boot = useLoad(() => api.bootTimes(), []);
  const [label, setLabel] = useState("");
  const [drive, setDrive] = useState<string>("");
  const [busy, setBusy] = useState<BenchSuite | null>(null);
  const [progress, setProgress] = useState<BenchProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<number | null>(null);
  const [baseline, setBaseline] = useState<Record<string, number>>({});

  useEffect(() => {
    if (!drive && roots.data?.length) setDrive(roots.data[0].path);
  }, [roots.data, drive]);

  const all = runs.data ?? [];
  const current = all.find((r) => r.id === view) ?? all[0] ?? null;
  const sameKind = (a: BenchRun, b: BenchRun) => a.suite === b.suite && (a.target ?? "") === (b.target ?? "");
  const candidates = current ? all.filter((r) => r.id !== current.id && sameKind(r, current)) : [];
  const groupKey = current ? `${current.suite}|${current.target ?? ""}` : "";
  // Default baseline: the oldest earlier run of the same kind.
  const base = useMemo(() => {
    if (!current) return null;
    const chosen = baseline[groupKey];
    if (chosen === -1) return null;
    return candidates.find((r) => r.id === chosen) ?? candidates.filter((r) => r.started_at < current.started_at).pop() ?? null;
  }, [current, candidates, baseline, groupKey]);

  async function start(suite: BenchSuite) {
    setBusy(suite);
    setError(null);
    setProgress({ stage: "Starting", pct: 0 });
    try {
      const r = await api.runBenchmark(suite, suite === "storage" ? drive : null, label.trim() || `${NAME[suite]}${suite === "storage" ? ` ${drive}` : ""}`, setProgress);
      await runs.reload();
      setView(r.id);
      toast({ tone: "good", title: `${NAME[suite]} benchmark finished`, detail: `${r.metrics.length} measurements saved` });
    } catch (e) {
      const msg = errorMessage(e);
      if (!msg.includes("cancelled")) setError(msg);
    } finally {
      setBusy(null);
      setProgress(null);
    }
  }

  async function remove(id: number) {
    try {
      await api.deleteBenchmark(id);
      if (view === id) setView(null);
      runs.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't delete", detail: errorMessage(e) });
    }
  }

  async function rename(r: BenchRun) {
    const next = window.prompt("Name this run", r.label);
    if (next == null) return;
    await api.renameBenchmark(r.id, next).catch((e) => toast({ tone: "critical", title: "Couldn't rename", detail: errorMessage(e) }));
    runs.reload();
  }

  return (
    <div className="page">
      <PageHead
        eyebrow="Benchmarks"
        title="Measure, change, measure again"
        desc="Run a benchmark before you change something (a profile, a driver, a BIOS setting) and again afterwards. NOCTURN shows what actually changed — and calls differences under 5% what they are: normal variation."
      />

      <Panel title="Run a benchmark">
        <div className="row" style={{ gap: 12, flexWrap: "wrap", marginBottom: 14 }}>
          <div className="field" style={{ minWidth: 280, flex: 1 }}>
            <label htmlFor="blabel">Name this run (optional)</label>
            <input id="blabel" className="input" placeholder="e.g. Before Gaming profile" value={label} maxLength={80} disabled={!!busy} onChange={(e) => setLabel(e.target.value)} />
          </div>
          <div className="field" style={{ minWidth: 200 }}>
            <label htmlFor="bdrive">Drive for the storage test</label>
            <select id="bdrive" className="select" value={drive} disabled={!!busy} onChange={(e) => setDrive(e.target.value)}>
              {(roots.data ?? []).map((r) => (
                <option key={r.path} value={r.path}>{r.label}{r.free != null ? ` — ${bytes(r.free)} free` : ""}</option>
              ))}
            </select>
          </div>
        </div>
        <div className="grid cols-4">
          {SUITES.map((s) => (
            <div key={s.id} className="panel" style={{ background: "var(--bg-1)" }}>
              <div className="stack" style={{ gap: 6, height: "100%" }}>
                <div className="row between">
                  <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>{s.name}</h3>
                  <span className="small dim">{s.time}</span>
                </div>
                <div className="small muted" style={{ flex: 1 }}>{s.desc}</div>
                {busy === s.id ? (
                  <div className="stack" style={{ gap: 6 }}>
                    <div className="small">{progress?.stage ?? "Running"}…</div>
                    <div className="meter" role="progressbar" aria-valuenow={Math.round(progress?.pct ?? 0)} aria-valuemin={0} aria-valuemax={100}>
                      <span style={{ width: `${progress?.pct ?? 0}%` }} />
                    </div>
                    <button className="btn sm" onClick={() => api.cancelBenchmark()}>Cancel</button>
                  </div>
                ) : (
                  <button className="btn" disabled={!!busy || (s.id === "storage" && !drive)} onClick={() => start(s.id)}>
                    Run <I.arrow />
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
        {error && <div className="mt-2"><ErrorNote message={error} /></div>}
        <div className="small dim mt-2">
          Close games and heavy apps first. A run is refused while a game is running. Frame-rate (FPS) measurement isn't included yet — it needs Intel's PresentMon.
        </div>
      </Panel>

      {current && (
        <Panel
          className="mt-2"
          title={`${current.label}`}
          sub={`${NAME[current.suite]}${current.target ? ` · ${current.target}` : ""} · ${date(current.started_at, true)}${current.context.power_plan ? ` · power plan: ${current.context.power_plan}` : ""}`}
          actions={
            candidates.length > 0 && (
              <div className="row" style={{ gap: 6 }}>
                <label htmlFor="bbase" className="small dim">Compare with</label>
                <select
                  id="bbase"
                  className="select"
                  style={{ width: 260 }}
                  value={base?.id ?? -1}
                  onChange={(e) => setBaseline((b) => ({ ...b, [groupKey]: Number(e.target.value) }))}
                >
                  <option value={-1}>No comparison</option>
                  {candidates.map((r) => (
                    <option key={r.id} value={r.id}>{r.label} — {date(r.started_at, true)}</option>
                  ))}
                </select>
              </div>
            )
          }
        >
          <Compare run={current} base={base} />
          {base && base.context.power_plan !== current.context.power_plan && (
            <div className="mt-1"><Note tone="warning">The power plan differed between these runs ({base.context.power_plan ?? "unknown"} → {current.context.power_plan ?? "unknown"}), which changes CPU results on its own.</Note></div>
          )}
          {current.context.notes.map((n) => <div key={n} className="small dim mt-1">{n}</div>)}
        </Panel>
      )}

      <Panel className="mt-2" title="All runs" sub="Click a run to see it; pick any earlier run of the same kind as the baseline.">
        {runs.error && <ErrorNote message={runs.error} onRetry={runs.reload} />}
        {runs.data && all.length === 0 && <Empty title="No benchmarks yet">Run one above to get a baseline.</Empty>}
        {all.length > 0 && (
          <div className="table-wrap" style={{ maxHeight: 360 }}>
            <table className="table">
              <thead><tr><th>Run</th><th>Suite</th><th>Headline</th><th style={{ width: 140 }}>When</th><th className="r" style={{ width: 150 }} /></tr></thead>
              <tbody>
                {all.map((r) => (
                  <tr key={r.id} onClick={() => setView(r.id)} style={{ background: current?.id === r.id ? "var(--moon-wash)" : undefined, cursor: "default" }}>
                    <td style={{ fontWeight: 500 }}>{r.label}</td>
                    <td className="small">{NAME[r.suite]}{r.target ? ` ${r.target}` : ""}</td>
                    <td className="small num">{r.metrics[0] ? `${r.metrics[0].label}: ${fmt(r.metrics[0].value, r.metrics[0].unit)}` : "—"}</td>
                    <td className="small" title={date(r.started_at, true)}>{relative(r.started_at)}</td>
                    <td className="r" style={{ whiteSpace: "nowrap" }}>
                      <button className="btn ghost sm" onClick={(e) => { e.stopPropagation(); rename(r); }}>Rename</button>
                      <button className="btn ghost sm icon" aria-label="Delete run" onClick={(e) => { e.stopPropagation(); remove(r.id); }}><I.close /></button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>

      <Panel className="mt-2" title="Recent boots" sub="From Windows' event log. Start-to-sign-in includes any time spent at the sign-in screen, so a password or PIN adds to it.">
        {boot.error && <ErrorNote message={boot.error} onRetry={boot.reload} />}
        {boot.data && (
          <div className="stack" style={{ gap: 10 }}>
            <dl className="kv">
              <dt>Firmware (BIOS/UEFI), last boot</dt>
              <dd>{boot.data.firmware_s != null ? `${boot.data.firmware_s.toFixed(1)} s` : "not recorded"}</dd>
              <dt>Typical start to sign-in</dt>
              <dd>{boot.data.median_to_sign_in != null ? `${boot.data.median_to_sign_in.toFixed(1)} s (median of cold boots)` : "—"}</dd>
              <dt>Fast startup</dt>
              <dd>{boot.data.fast_startup == null ? "unknown" : boot.data.fast_startup ? "on — shutdowns hibernate the kernel, so starts are quicker" : "off — every start is a full cold boot"}</dd>
            </dl>
            {boot.data.boots.length > 0 && (
              <table className="table">
                <thead><tr><th>Started</th><th>Type</th><th className="r">Start to sign-in</th></tr></thead>
                <tbody>
                  {boot.data.boots.slice(0, 10).map((b) => (
                    <tr key={b.start}>
                      <td className="small">{date(b.start, true)}</td>
                      <td className="small muted">{b.boot_type === 0 ? "Cold boot" : b.boot_type === 1 ? "Fast startup" : b.boot_type === 2 ? "Resume from hibernation" : "—"}</td>
                      <td className="r num">{b.to_sign_in != null ? `${b.to_sign_in.toFixed(1)} s` : "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {boot.data.firmware_s != null && boot.data.firmware_s > 15 && (
              <Note>
                Firmware takes {boot.data.firmware_s.toFixed(0)} s before Windows even starts. Common causes: memory training after a BIOS change, a boot-device scan of empty drives, or "fast boot" being off in the BIOS. Startup apps don't affect this part — see{" "}
                <a href="#" onClick={(e) => { e.preventDefault(); navigate("startup"); }}>Startup Manager</a> for what happens after sign-in.
              </Note>
            )}
          </div>
        )}
      </Panel>
    </div>
  );
}
