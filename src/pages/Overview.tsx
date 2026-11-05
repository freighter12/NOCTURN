import { useState } from "react";
import { LineChart } from "../components/LineChart";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Meter, PageHead, Panel, Spinner, StatusTag } from "../components/ui";
import { api, type Finding, type HealthReport } from "../lib/api";
import { ToneIcon, useApp, useLoad } from "../lib/app";
import { bytes, bytesParts, duration, pct, relative } from "../lib/format";
import { primaryGpu, useTelemetry } from "../lib/telemetry";

const ASK = ["Why is my PC slower than usual?", "Optimise my PC for gaming.", "Find what's using my RAM.", "Make Windows boot faster."];

export function Overview() {
  const { navigate, requestApply, dataVersion } = useApp();
  const { latest, history } = useTelemetry();
  const sys = useLoad(() => api.systemInfo(), []);
  const health = useLoad(() => api.health(false), [dataVersion]);
  const txs = useLoad(() => api.history(5), [dataVersion]);
  const plans = useLoad(() => api.powerPlans(), [dataVersion]);
  const [q, setQ] = useState("");
  const [rechecking, setRechecking] = useState(false);

  const times = history.map((s) => s.ts);
  const gpu = primaryGpu(latest);
  const gpuIdx = latest && gpu ? latest.gpus.indexOf(gpu) : 0;
  const memPct = latest ? (latest.mem_used / Math.max(1, latest.mem_total)) * 100 : 0;
  const activePlan = plans.data?.find((p) => p.active);

  function act(f: Finding) {
    const a = f.action;
    if (!a) return;
    if (a.kind === "tweaks") requestApply({ tweakIds: a.ids, source: "user", label: a.label, title: a.label });
    else if (a.kind === "page") navigate(a.page as never);
    else navigate("assistant", { prompt: a.prompt });
  }

  async function recheck() {
    setRechecking(true);
    try {
      health.setData(await api.health(true));
    } finally {
      setRechecking(false);
    }
  }

  function ask(text: string) {
    if (!text.trim()) return;
    navigate("assistant", { prompt: text.trim() });
  }

  return (
    <div className="page">
      <PageHead
        eyebrow="Overview"
        title={sys.data?.os.hostname ?? "Your PC"}
        desc={
          sys.data
            ? `${sys.data.os.name}${sys.data.os.display_version ? ` ${sys.data.os.display_version}` : ""} · build ${sys.data.os.build}${sys.data.os.ubr ? `.${sys.data.os.ubr}` : ""} · ${sys.data.is_laptop ? "Laptop" : "Desktop"}`
            : "Reading system information…"
        }
        actions={
          <button className="btn" onClick={recheck} disabled={rechecking}>
            {rechecking ? <Spinner /> : <I.refresh />} Run health check
          </button>
        }
      />

      <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1.05fr) minmax(0, 1fr)" }}>
        <Panel lit className="condition">
          <div className="moon-halo" />
          <div className="row" style={{ gap: 26, alignItems: "center", position: "relative" }}>
            <ConditionRing report={health.data} loading={health.loading && !health.data} />
            <div style={{ minWidth: 0 }}>
              <div className="eyebrow dim small" style={{ letterSpacing: ".16em", textTransform: "uppercase", fontSize: 10.5 }}>
                System condition
              </div>
              <div style={{ font: "600 24px/1.2 var(--font-display)", marginTop: 4 }}>{health.data?.condition ?? "Checking…"}</div>
              <div className="muted mt-1" style={{ maxWidth: 360 }}>
                {health.data ? summarise(health.data) : "Looking at storage, memory, startup, drivers and the event log."}
              </div>
              {health.data && <div className="dim small mt-1">Checked {relative(health.data.checked_at)}</div>}
            </div>
          </div>
        </Panel>

        <Panel lit>
          <div className="panel-head">
            <div>
              <h2>Ask NOCTURN AI</h2>
              <div className="sub">Describe a problem in your own words. It inspects this PC before answering, and changes nothing without your approval.</div>
            </div>
          </div>
          <form
            className="ai-entry"
            onSubmit={(e) => {
              e.preventDefault();
              ask(q);
            }}
          >
            <I.assistant style={{ width: 16, height: 16, color: "var(--moon-2)" }} />
            <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="e.g. My game stutters every few seconds" aria-label="Ask NOCTURN AI" />
            <button className="btn primary sm" type="submit" disabled={!q.trim()}>
              Ask
            </button>
          </form>
          <div className="chips mt-2">
            {ASK.map((a) => (
              <button key={a} className="chip" onClick={() => ask(a)}>
                {a}
              </button>
            ))}
          </div>
        </Panel>
      </div>

      <div className="grid cols-3 mt-2">
        <Panel title="Processor" sub={sys.data?.cpu.name ?? "—"}>
          <Big value={latest ? latest.cpu_usage.toFixed(0) : "—"} unit="%" foot={latest ? `${(latest.cpu_freq_mhz / 1000).toFixed(2)} GHz · ${latest.cpu_per_core.length} threads` : ""} />
          <LineChart ariaLabel="CPU usage, last two minutes" series={[{ key: "cpu", label: "CPU", color: "var(--series-1)", values: history.map((s) => s.cpu_usage) }]} times={times} max={100} format={(v) => `${Math.round(v)}%`} height={96} />
        </Panel>
        <Panel title="Graphics" sub={gpu?.name ?? sys.data?.gpus[0]?.name ?? "—"}>
          <Big
            value={gpu?.usage != null ? gpu.usage.toFixed(0) : "—"}
            unit={gpu?.usage != null ? "%" : ""}
            foot={
              gpu
                ? [gpu.vram_total ? `VRAM ${bytes(gpu.vram_used)} / ${bytes(gpu.vram_total)}` : null, gpu.temperature_c != null ? `${gpu.temperature_c.toFixed(0)} °C` : "Temperature not exposed"]
                    .filter(Boolean)
                    .join(" · ")
                : "No GPU telemetry"
            }
          />
          <LineChart ariaLabel="GPU usage, last two minutes" series={[{ key: "gpu", label: "GPU", color: "var(--series-1)", values: history.map((s) => s.gpus[gpuIdx]?.usage ?? null) }]} times={times} max={100} format={(v) => `${Math.round(v)}%`} height={96} />
        </Panel>
        <Panel title="Memory" sub={sys.data?.memory_modules.length ? `${sys.data.memory_modules.length} module(s)${sys.data.memory_modules[0]?.configured_mts ? ` · ${sys.data.memory_modules[0].configured_mts} MT/s` : ""}` : "—"}>
          <Big value={latest ? bytesParts(latest.mem_used)[0] : "—"} unit={latest ? `${bytesParts(latest.mem_used)[1]} of ${bytes(latest.mem_total)}` : ""} foot={latest ? `${pct(memPct)} in use · ${bytes(latest.mem_available)} available` : ""} />
          <LineChart ariaLabel="Memory in use, last two minutes" series={[{ key: "mem", label: "Memory", color: "var(--series-1)", values: history.map((s) => (s.mem_used / Math.max(1, s.mem_total)) * 100) }]} times={times} max={100} format={(v) => `${Math.round(v)}%`} height={96} />
        </Panel>
      </div>

      <div className="grid mt-2" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 2fr)" }}>
        <div className="grid">
          <Panel title="Storage">
            {!latest && <div className="skeleton" style={{ height: 60 }} />}
            <div className="stack" style={{ gap: 14 }}>
              {latest?.disks.filter((d) => !d.removable).map((d) => {
                const used = d.total - d.available;
                const p = (used / d.total) * 100;
                return (
                  <div key={d.mount} className="stack" style={{ gap: 6 }}>
                    <div className="row between small">
                      <span>
                        <b style={{ fontWeight: 600 }}>{d.mount.replace(/\\$/, "")}</b> <span className="dim">{d.label || d.file_system}</span>
                      </span>
                      <span className="muted num">{bytes(d.available)} free of {bytes(d.total)}</span>
                    </div>
                    <Meter value={p} warnAt={85} critAt={95} label={`${d.mount} used`} />
                  </div>
                );
              })}
            </div>
          </Panel>
          <Panel title="At a glance">
            <dl className="kv">
              <dt>Uptime</dt>
              <dd>{latest ? duration(latest.uptime_secs) : "—"}</dd>
              <dt>Power plan</dt>
              <dd>{activePlan?.name ?? "—"}</dd>
              <dt>Processes</dt>
              <dd>{latest?.process_count || "—"}</dd>
              <dt>Network</dt>
              <dd>{latest ? `↓ ${bytes(latest.net_rx_bps)}/s · ↑ ${bytes(latest.net_tx_bps)}/s` : "—"}</dd>
            </dl>
          </Panel>
        </div>

        <Panel title="Detected issues & recommendations" sub={health.data ? `${health.data.findings.length} finding(s), each based on measured evidence` : undefined}>
          {health.error && <ErrorNote message={health.error} onRetry={health.reload} />}
          {health.loading && !health.data && (
            <div className="stack">
              {[0, 1, 2].map((i) => (
                <div key={i} className="skeleton" style={{ height: 44 }} />
              ))}
            </div>
          )}
          {health.data && health.data.findings.length === 0 && <Empty title="No issues found">Storage, memory, startup, drivers and recent errors all look healthy.</Empty>}
          <div className="list">
            {health.data?.findings.slice(0, 7).map((f) => (
              <div key={f.id} className="list-item">
                <span style={{ color: f.severity === "critical" ? "var(--critical)" : f.severity === "warning" ? "var(--warning)" : "var(--ink-2)", width: 16, height: 16, marginTop: 2 }}>
                  <ToneIcon tone={f.severity === "info" ? "info" : "warning"} />
                </span>
                <div className="body">
                  <div className="title">
                    {f.title} <span className={`tag ${f.severity === "critical" ? "critical" : f.severity === "warning" ? "warning" : ""}`} style={{ marginLeft: 6 }}>{f.severity === "info" ? "Info" : f.severity === "warning" ? "Warning" : "Critical"}</span>
                  </div>
                  <div className="desc">{f.detail}</div>
                  <div className="small dim mt-1 selectable">Evidence: {f.evidence}</div>
                </div>
                {f.action && (
                  <button className="btn sm" onClick={() => act(f)}>
                    {f.action.label}
                  </button>
                )}
              </div>
            ))}
          </div>
          {health.data && health.data.skipped.length > 0 && <div className="small dim mt-2">Not checked: {health.data.skipped.join(", ")}</div>}
        </Panel>
      </div>

      <Panel title="Recent optimisation activity" className="mt-2" actions={<button className="btn ghost sm" onClick={() => navigate("history")}>View all <I.arrow /></button>}>
        {txs.data && txs.data.length === 0 && <Empty title="No changes yet">Everything NOCTURN changes is recorded here with its original value, so you can roll it back.</Empty>}
        <div className="list">
          {txs.data?.map((t) => (
            <div key={t.id} className="list-item" style={{ alignItems: "center" }}>
              <div className="body">
                <div className="title">{t.label}</div>
                <div className="desc">
                  {relative(t.created_at)} · {t.source === "ai" ? "Approved from AI plan" : t.source.startsWith("profile:") ? "Profile" : "Manual"} · {t.applied} applied{t.failed ? ` · ${t.failed} failed` : ""}
                </div>
              </div>
              <StatusTag status={t.status} />
            </div>
          ))}
        </div>
      </Panel>
    </div>
  );
}

function Big({ value, unit, foot }: { value: string; unit?: string; foot?: string }) {
  return (
    <div className="stat" style={{ marginBottom: 10 }}>
      <div className="value">
        {value}
        {unit && <span className="unit">{unit}</span>}
      </div>
      {foot && <div className="foot">{foot}</div>}
    </div>
  );
}

function summarise(r: HealthReport): string {
  const c = r.findings.filter((f) => f.severity === "critical").length;
  const w = r.findings.filter((f) => f.severity === "warning").length;
  const i = r.findings.filter((f) => f.severity === "info").length;
  if (!r.findings.length) return "No problems detected in storage, memory, startup, drivers or the event log.";
  const parts = [c && `${c} critical`, w && `${w} warning${w > 1 ? "s" : ""}`, i && `${i} suggestion${i > 1 ? "s" : ""}`].filter(Boolean);
  return `${parts.join(", ")}. Details and fixes are listed below.`;
}

function ConditionRing({ report, loading }: { report: HealthReport | null; loading: boolean }) {
  const score = report?.score ?? 0;
  const r = 58;
  const c = 2 * Math.PI * r;
  const color = !report ? "var(--ink-3)" : report.condition === "Good" ? "var(--moon)" : report.condition === "Fair" ? "var(--warning)" : "var(--critical)";
  return (
    <div className="condition-ring" role="img" aria-label={report ? `Health score ${score} of 100, ${report.condition}` : "Checking"}>
      <svg viewBox="0 0 132 132">
        <circle cx="66" cy="66" r={r} fill="none" stroke="var(--bg-4)" strokeWidth="5" />
        <circle
          cx="66"
          cy="66"
          r={r}
          fill="none"
          stroke={color}
          strokeWidth="5"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={c * (1 - score / 100)}
          style={{ transition: "stroke-dashoffset 900ms var(--ease)", filter: report?.condition === "Good" ? "drop-shadow(0 0 6px rgba(223,226,232,.35))" : undefined }}
        />
      </svg>
      <div className="center">
        <div>
          {loading ? <Spinner lg /> : <b>{report ? score : "—"}</b>}
          <span>of 100</span>
        </div>
      </div>
    </div>
  );
}
