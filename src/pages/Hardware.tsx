// Hardware monitor: live CPU/GPU/memory/storage readings, 7-day history,
// refresh rate and pause, and warning thresholds. Every number shown comes
// from Windows or the GPU vendor's library; anything unavailable says why.

import { useEffect, useState } from "react";
import { LineChart } from "../components/LineChart";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Meter, Note, PageHead, Panel, Seg, Spinner, Toggle } from "../components/ui";
import { api, errorMessage, type AppSettings, type GpuSample, type HistoryPoint, type MonitorSettings } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { bytes, pct, rate } from "../lib/format";
import { primaryGpu, useTelemetry } from "../lib/telemetry";

type Range = "60" | "1440" | "10080";

const bps = (v: number) => `${bytes(v)}/s`;

export function Hardware() {
  const { settings, setSettings, toast } = useApp();
  const { latest, history } = useTelemetry();
  const sys = useLoad(() => api.systemInfo(), []);
  const drives = useLoad(() => api.storageHealth(), []);
  const [range, setRange] = useState<Range>("60");
  const hist = useLoad(() => api.telemetryRange(Number(range)), [range]);
  const [saving, setSaving] = useState(false);
  const mon = settings?.monitor;

  // Refresh the history view once a minute while it's open.
  useEffect(() => {
    const t = setInterval(hist.reload, 60_000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [range]);

  async function saveMonitor(patch: Partial<MonitorSettings>) {
    if (!settings) return;
    setSaving(true);
    try {
      const next: AppSettings = { ...settings, monitor: { ...settings.monitor, ...patch } };
      setSettings(await api.saveSettings(next));
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save monitoring settings", detail: errorMessage(e) });
    } finally {
      setSaving(false);
    }
  }

  const times = history.map((s) => s.ts);
  const gpu = primaryGpu(latest);
  const gpuIdx = latest && gpu ? latest.gpus.indexOf(gpu) : 0;
  const memPct = latest ? (latest.mem_used / Math.max(1, latest.mem_total)) * 100 : 0;
  const commitPct = latest?.commit_bytes && latest.commit_limit ? (latest.commit_bytes / latest.commit_limit) * 100 : null;
  const modules = sys.data?.memory_modules ?? [];

  return (
    <div className="page">
      <PageHead
        eyebrow="Hardware"
        title="Hardware monitor"
        desc="Live readings from Windows and your GPU driver, with a 7-day history. Sensors Windows doesn't expose are marked as unavailable rather than estimated."
        actions={
          mon && (
            <>
              <Seg<string>
                value={String(mon.interval_ms)}
                onChange={(v) => saveMonitor({ interval_ms: Number(v) })}
                options={[{ value: "1000", label: "1 s" }, { value: "2000", label: "2 s" }, { value: "5000", label: "5 s" }]}
              />
              <button className={`btn ${mon.paused ? "primary" : ""}`} disabled={saving} onClick={() => saveMonitor({ paused: !mon.paused })}>
                {saving ? <Spinner /> : mon.paused ? <I.bolt /> : <I.stop />} {mon.paused ? "Resume monitoring" : "Pause"}
              </button>
            </>
          )
        }
      />

      {mon?.paused && (
        <div className="mb-2" style={{ marginBottom: 14 }}>
          <Note tone="warning">Monitoring is paused. Readings below are frozen and no history is recorded. Saved process rules still apply, checked every 10 seconds.</Note>
        </div>
      )}

      <div className="grid cols-2">
        {/* CPU */}
        <Panel title="Processor" sub={sys.data?.cpu.name ?? "—"}>
          <div className="hw-figures">
            <Fig label="Usage" value={latest ? pct(latest.cpu_usage) : "—"} />
            <Fig label="Busiest core" value={latest ? pct(Math.max(...latest.cpu_per_core)) : "—"} />
            <Fig label="Clock" value={latest ? `${(latest.cpu_freq_mhz / 1000).toFixed(2)} GHz` : "—"} />
            <Fig label="Cores / threads" value={sys.data ? `${sys.data.cpu.cores ?? "—"} / ${sys.data.cpu.threads ?? "—"}` : "—"} />
          </div>
          <div className="cores mt-1" style={{ marginBottom: 14 }} role="img" aria-label="Per-core CPU load">
            {(latest?.cpu_per_core ?? []).map((v, i) => (
              <span key={i} style={{ height: `${Math.max(3, v)}%` }} title={`CPU ${i}: ${v.toFixed(0)}%`} />
            ))}
          </div>
          <LineChart ariaLabel="CPU usage, recent" series={[{ key: "cpu", label: "CPU", color: "var(--series-1)", values: history.map((s) => s.cpu_usage) }]} times={times} max={100} format={(v) => `${Math.round(v)}%`} height={90} />
          <div className="small dim mt-1">Temperature and power: not available. Windows has no standard CPU sensor API, and reading them would need a kernel driver NOCTURN deliberately doesn't install.</div>
        </Panel>

        {/* GPU */}
        <Panel title="Graphics" sub={gpu?.name ?? sys.data?.gpus[0]?.name ?? "—"} actions={gpu && <span className="tag" title={gpu.source === "nvml" ? "Read from NVIDIA's driver library (NVML)" : "Usage and memory from Windows; no vendor sensors"}>{gpu.source === "nvml" ? "NVIDIA sensors" : "Windows counters"}</span>}>
          {!gpu ? (
            <Empty title="No GPU telemetry" />
          ) : (
            <>
              <div className="hw-figures">
                <Fig label="Usage" value={gpu.usage != null ? pct(gpu.usage) : "—"} />
                <Fig label="Temperature" value={gpu.temperature_c != null ? `${gpu.temperature_c.toFixed(0)} °C` : "n/a"} warn={gpu.temperature_c != null && mon ? gpu.temperature_c >= mon.gpu_temp_alert_c : false} />
                <Fig label="Power" value={gpu.power_w != null ? `${gpu.power_w.toFixed(0)} W` : "n/a"} sub={gpu.power_limit_w ? `of ${gpu.power_limit_w.toFixed(0)} W` : undefined} />
                <Fig label="Fan" value={gpu.fan_pct != null ? `${gpu.fan_pct}%` : "n/a"} />
              </div>
              <GpuDetails g={gpu} />
              <LineChart
                ariaLabel="GPU usage and temperature, recent"
                series={[
                  { key: "use", label: "Usage %", color: "var(--series-1)", values: history.map((s) => s.gpus[gpuIdx]?.usage ?? null) },
                  { key: "temp", label: "Temp °C", color: "var(--series-2)", values: history.map((s) => s.gpus[gpuIdx]?.temperature_c ?? null) },
                ]}
                times={times}
                max={100}
                format={(v) => `${Math.round(v)}`}
                height={90}
              />
            </>
          )}
        </Panel>

        {/* Memory */}
        <Panel title="Memory" sub={latest ? `${bytes(latest.mem_total)} installed${modules[0]?.configured_mts ? ` · ${modules[0].configured_mts} MT/s` : ""}` : "—"}>
          <div className="hw-figures">
            <Fig label="In use" value={latest ? bytes(latest.mem_used) : "—"} sub={latest ? pct(memPct) : undefined} warn={!!mon && memPct >= mon.ram_alert_pct} />
            <Fig label="Available" value={latest ? bytes(latest.mem_available) : "—"} />
            <Fig label="Committed" value={latest?.commit_bytes ? bytes(latest.commit_bytes) : "—"} sub={latest?.commit_limit ? `of ${bytes(latest.commit_limit)}` : undefined} />
            <Fig label="Hard faults" value={latest?.pages_per_sec != null ? `${Math.round(latest.pages_per_sec)}/s` : "—"} />
          </div>
          {commitPct != null && (
            <div className="stack mt-1" style={{ gap: 6 }}>
              <div className="row between small">
                <span className="muted">Memory pressure (commit charge)</span>
                <span className="num">{pct(commitPct)}</span>
              </div>
              <Meter value={commitPct} warnAt={80} critAt={92} label="Commit charge" />
              <div className="small dim">Above ~90%, Windows runs short of memory to promise apps. Sustained hard faults in the hundreds mean it's reading memory back from disk, which causes stutter.</div>
            </div>
          )}
          <LineChart ariaLabel="Memory in use, recent" series={[{ key: "mem", label: "Memory", color: "var(--series-1)", values: history.map((s) => (s.mem_used / Math.max(1, s.mem_total)) * 100) }]} times={times} max={100} format={(v) => `${Math.round(v)}%`} height={70} />
        </Panel>

        {/* Storage I/O */}
        <Panel title="Storage activity" sub="Per physical disk">
          {!latest?.disk_io.length && <div className="small muted">Disk counters aren't available on this PC.</div>}
          <div className="stack" style={{ gap: 12 }}>
            {latest?.disk_io.map((d) => (
              <div key={d.disk} className="stack" style={{ gap: 6 }}>
                <div className="row between small">
                  <span><b style={{ fontWeight: 600 }}>Disk {d.disk}</b></span>
                  <span className="muted num">R {bps(d.read_bps)} · W {bps(d.write_bps)} · {pct(d.active_pct)} busy</span>
                </div>
                <Meter value={d.active_pct} warnAt={80} critAt={95} label={`Disk ${d.disk} busy`} />
              </div>
            ))}
          </div>
          <LineChart
            ariaLabel="Disk throughput, recent"
            series={[
              { key: "r", label: "Read", color: "var(--series-1)", values: history.map((s) => s.disk_io.reduce((a, d) => a + d.read_bps, 0)) },
              { key: "w", label: "Write", color: "var(--series-2)", values: history.map((s) => s.disk_io.reduce((a, d) => a + d.write_bps, 0)) },
            ]}
            times={times}
            format={bps}
            height={80}
          />
        </Panel>
      </div>

      <DrivesPanel loading={drives.loading} error={drives.error} reload={drives.reload} data={drives.data} />

      <Panel
        className="mt-2"
        title="History"
        sub="One averaged reading per minute, kept for 7 days. Recorded only while NOCTURN is running and monitoring isn't paused."
        actions={<Seg<Range> value={range} onChange={setRange} options={[{ value: "60", label: "1 hour" }, { value: "1440", label: "24 hours" }, { value: "10080", label: "7 days" }]} />}
      >
        {hist.error && <ErrorNote message={hist.error} onRetry={hist.reload} />}
        {hist.loading && !hist.data && <Loading label="Loading history…" />}
        {hist.data && hist.data.length < 2 && <Empty title="Not enough history yet">History builds up one point per minute while NOCTURN runs.</Empty>}
        {hist.data && hist.data.length >= 2 && <HistoryCharts points={hist.data} />}
      </Panel>

      {mon && <AlertsPanel mon={mon} saving={saving} save={saveMonitor} />}
    </div>
  );
}

function Fig({ label, value, sub, warn }: { label: string; value: string; sub?: string; warn?: boolean }) {
  return (
    <div className="hw-fig">
      <div className="l">{label}</div>
      <div className="v num" style={{ color: warn ? "var(--warning)" : undefined }}>{value}</div>
      {sub && <div className="s">{sub}</div>}
    </div>
  );
}

function GpuDetails({ g }: { g: GpuSample }) {
  return (
    <dl className="kv mt-1 mb-1" style={{ marginBottom: 10 }}>
      <dt>Video memory</dt>
      <dd className="num">{g.vram_total ? `${bytes(g.vram_used)} of ${bytes(g.vram_total)}` : "—"}</dd>
      <dt>Core clock</dt>
      <dd className="num">{g.core_clock_mhz != null ? `${g.core_clock_mhz} MHz${g.max_core_clock_mhz ? ` (max ${g.max_core_clock_mhz})` : ""}` : "n/a"}</dd>
      <dt>Memory clock</dt>
      <dd className="num">{g.mem_clock_mhz != null ? `${g.mem_clock_mhz} MHz` : "n/a"}</dd>
      {g.source !== "nvml" && (
        <>
          <dt>Sensors</dt>
          <dd className="muted">Temperature, clocks and power need the GPU vendor's library. NOCTURN currently reads them on NVIDIA cards only.</dd>
        </>
      )}
    </dl>
  );
}

function DrivesPanel({ data, loading, error, reload }: { data: Awaited<ReturnType<typeof api.storageHealth>> | null; loading: boolean; error: string | null; reload: () => void }) {
  return (
    <Panel className="mt-2" title="Drives" sub="Model, health and reliability counters reported by Windows" actions={<button className="btn sm icon" onClick={reload} aria-label="Refresh drives"><I.refresh /></button>}>
      {loading && !data && <Loading label="Reading drive health…" />}
      {error && <ErrorNote message={error} onRetry={reload} />}
      {data && (
        <div className="stack" style={{ gap: 10 }}>
          <table className="table">
            <thead>
              <tr>
                <th>Model</th>
                <th>Type</th>
                <th className="r">Size</th>
                <th>Health</th>
                <th className="r">Temp</th>
                <th className="r">Wear</th>
                <th className="r">Powered on</th>
              </tr>
            </thead>
            <tbody>
              {data.drives.map((d) => (
                <tr key={d.device_id}>
                  <td>{d.model}</td>
                  <td className="small">{[d.media_type, d.bus_type].filter(Boolean).join(" · ") || "—"}</td>
                  <td className="r num">{bytes(d.size_bytes)}</td>
                  <td><span className={`tag ${d.health === "Healthy" ? "good" : d.health ? "warning" : ""}`}>{d.health ?? "Unknown"}</span></td>
                  <td className="r num">{d.temperature_c != null ? `${d.temperature_c} °C` : "—"}</td>
                  <td className="r num">{d.wear_pct != null ? `${d.wear_pct}%` : "—"}</td>
                  <td className="r num">{d.power_on_hours != null ? `${d.power_on_hours.toLocaleString()} h` : "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {data.note && <Note>{data.note}</Note>}
        </div>
      )}
    </Panel>
  );
}

function HistoryCharts({ points }: { points: HistoryPoint[] }) {
  const times = points.map((p) => p.ts);
  const span = times[times.length - 1] - times[0];
  const label = (ts: number) =>
    new Date(ts).toLocaleString(undefined, span > 36 * 3600_000 ? { weekday: "short", hour: "2-digit", minute: "2-digit" } : { hour: "2-digit", minute: "2-digit" });
  const hasGpu = points.some((p) => p.gpu != null);
  const hasTemp = points.some((p) => p.gpu_temp != null);
  return (
    <div className="grid cols-2">
      <div>
        <div className="small muted" style={{ marginBottom: 6 }}>CPU — average and busiest core</div>
        <LineChart
          ariaLabel="CPU history"
          series={[
            { key: "avg", label: "Average", color: "var(--series-1)", values: points.map((p) => p.cpu) },
            { key: "max", label: "Busiest core", color: "var(--series-2)", values: points.map((p) => p.cpu_max_core) },
          ]}
          times={times}
          max={100}
          format={(v) => `${Math.round(v)}%`}
          timeLabel={label}
          height={110}
        />
      </div>
      <div>
        <div className="small muted" style={{ marginBottom: 6 }}>Memory in use</div>
        <LineChart ariaLabel="Memory history" series={[{ key: "mem", label: "Memory", color: "var(--series-1)", values: points.map((p) => p.mem_pct) }]} times={times} max={100} format={(v) => `${Math.round(v)}%`} timeLabel={label} height={110} />
      </div>
      {hasGpu && (
        <div>
          <div className="small muted" style={{ marginBottom: 6 }}>GPU usage{hasTemp ? " and peak temperature" : ""}</div>
          <LineChart
            ariaLabel="GPU history"
            series={[
              { key: "gpu", label: "Usage %", color: "var(--series-1)", values: points.map((p) => p.gpu) },
              ...(hasTemp ? [{ key: "t", label: "Peak °C", color: "var(--series-2)", values: points.map((p) => p.gpu_temp) }] : []),
            ]}
            times={times}
            max={100}
            format={(v) => `${Math.round(v)}`}
            timeLabel={label}
            height={110}
          />
        </div>
      )}
      <div>
        <div className="small muted" style={{ marginBottom: 6 }}>Network</div>
        <LineChart
          ariaLabel="Network history"
          series={[
            { key: "rx", label: "Download", color: "var(--series-1)", values: points.map((p) => p.net_rx) },
            { key: "tx", label: "Upload", color: "var(--series-2)", values: points.map((p) => p.net_tx) },
          ]}
          times={times}
          format={rate}
          timeLabel={label}
          height={110}
        />
      </div>
    </div>
  );
}

function AlertsPanel({ mon, saving, save }: { mon: MonitorSettings; saving: boolean; save: (p: Partial<MonitorSettings>) => void }) {
  const [draft, setDraft] = useState(mon);
  useEffect(() => setDraft(mon), [mon]);
  const dirty = draft.cpu_alert_pct !== mon.cpu_alert_pct || draft.ram_alert_pct !== mon.ram_alert_pct || draft.gpu_temp_alert_c !== mon.gpu_temp_alert_c;
  const field = (key: "cpu_alert_pct" | "ram_alert_pct" | "gpu_temp_alert_c", label: string, unit: string, min: number, max: number) => (
    <div className="field">
      <label htmlFor={key}>{label}</label>
      <div className="row">
        <input id={key} className="input num" type="number" min={min} max={max} value={draft[key]} disabled={!mon.alerts} onChange={(e) => setDraft({ ...draft, [key]: Number(e.target.value) })} style={{ width: 110 }} />
        <span className="small muted">{unit}</span>
      </div>
    </div>
  );
  return (
    <Panel
      className="mt-2"
      title="Warnings"
      sub="NOCTURN notifies you when a reading stays above its limit for 15 seconds, at most once every 15 minutes per warning."
      actions={<Toggle on={mon.alerts} disabled={saving} onChange={(v) => save({ alerts: v })} label="Hardware warnings" />}
    >
      <div className="row wrap" style={{ gap: 24, alignItems: "flex-end" }}>
        {field("cpu_alert_pct", "CPU usage", "%", 50, 100)}
        {field("ram_alert_pct", "Memory in use", "%", 50, 100)}
        {field("gpu_temp_alert_c", "GPU temperature", "°C", 50, 110)}
        <button className="btn" disabled={!dirty || saving || !mon.alerts} onClick={() => save({ cpu_alert_pct: draft.cpu_alert_pct, ram_alert_pct: draft.ram_alert_pct, gpu_temp_alert_c: draft.gpu_temp_alert_c })}>
          Save limits
        </button>
      </div>
    </Panel>
  );
}
