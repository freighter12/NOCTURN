// System Diagnostics: full hardware/OS inventory, live per-core load, the
// Windows event log, drivers, services (read-only) and restore points.

import { useMemo, useState } from "react";
import { Crashes } from "../components/Crashes";
import { DriverCentre } from "../components/DriverCentre";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Seg, Spinner } from "../components/ui";
import { api, type EventEntry, type SystemInfo } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { bytes, date, duration, relative } from "../lib/format";
import { useTelemetry } from "../lib/telemetry";

type Tab = "crashes" | "hardware" | "events" | "drivers" | "services" | "restore";

export function Diagnostics() {
  const [tab, setTab] = useState<Tab>("hardware");
  const { navigate } = useApp();
  return (
    <div className="page">
      <PageHead
        eyebrow="System Diagnostics"
        title="Diagnostics"
        desc="Everything here is read directly from Windows. Where a reading isn't exposed by Windows or your drivers, NOCTURN says so instead of estimating."
        actions={
          <button className="btn" onClick={() => navigate("assistant", { prompt: "Analyse my recent system errors and driver information. Is anything wrong with this PC?" })}>
            <I.assistant /> Ask AI to analyse
          </button>
        }
      />
      <Seg<Tab>
        value={tab}
        onChange={setTab}
        options={[
          { value: "crashes", label: "Crashes" },
          { value: "hardware", label: "Hardware & Windows" },
          { value: "events", label: "Event log" },
          { value: "drivers", label: "Drivers" },
          { value: "services", label: "Services" },
          { value: "restore", label: "Restore points" },
        ]}
      />
      <div className="mt-2">
        {tab === "crashes" && <Crashes />}
        {tab === "hardware" && <Hardware />}
        {tab === "events" && <Events />}
        {tab === "drivers" && (
          <div className="stack" style={{ gap: 14 }}>
            <DriverCentre />
            <Drivers />
          </div>
        )}
        {tab === "services" && <Services />}
        {tab === "restore" && <Restore />}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- hardware

function Hardware() {
  const [refreshing, setRefreshing] = useState(false);
  const sys = useLoad(() => api.systemInfo(), []);
  const { latest } = useTelemetry();

  async function refresh() {
    setRefreshing(true);
    try {
      sys.setData(await api.systemInfo(true));
    } finally {
      setRefreshing(false);
    }
  }

  if (sys.loading && !sys.data) return <Loading label="Collecting hardware inventory…" />;
  if (sys.error || !sys.data) return <ErrorNote message={sys.error ?? "No data"} onRetry={sys.reload} />;
  const s: SystemInfo = sys.data;
  const or = (v: string | number | null | undefined, suffix = "") => (v == null || v === "" ? "—" : `${v}${suffix}`);

  return (
    <div className="stack" style={{ gap: 14 }}>
      <div className="row between">
        <span className="small dim">Collected {relative(s.collected_at)}</span>
        <button className="btn sm" onClick={refresh} disabled={refreshing}>
          {refreshing ? <Spinner /> : <I.refresh />} Re-scan
        </button>
      </div>
      {s.warnings.length > 0 && <Note tone="warning">Some details couldn't be read: {s.warnings.join("; ")}</Note>}

      <div className="grid cols-2">
        <Panel title="Windows">
          <dl className="kv">
            <dt>Edition</dt>
            <dd>{s.os.name}{s.os.edition ? ` (${s.os.edition})` : ""}</dd>
            <dt>Version</dt>
            <dd>{or(s.os.display_version)} · build {s.os.build}{s.os.ubr ? `.${s.os.ubr}` : ""}</dd>
            <dt>Architecture</dt>
            <dd>{or(s.os.architecture)}</dd>
            <dt>Installed</dt>
            <dd>{date(s.os.install_date)}</dd>
            <dt>Last boot</dt>
            <dd>{date(s.os.last_boot, true)}{latest ? ` · up ${duration(latest.uptime_secs)}` : ""}</dd>
            <dt>Computer name</dt>
            <dd>{or(s.os.hostname)}</dd>
            <dt>NOCTURN rights</dt>
            <dd>{s.is_elevated ? "Administrator" : "Standard user (elevates per change)"}</dd>
          </dl>
        </Panel>
        <Panel title="System & motherboard">
          <dl className="kv">
            <dt>System</dt>
            <dd>{[s.board.system_manufacturer, s.board.system_model].filter(Boolean).join(" ") || "—"}</dd>
            <dt>Motherboard</dt>
            <dd>{[s.board.board_manufacturer, s.board.board_product].filter(Boolean).join(" ") || "—"}</dd>
            <dt>BIOS / UEFI</dt>
            <dd>{[s.board.bios_vendor, s.board.bios_version].filter(Boolean).join(" ") || "—"}{s.board.bios_date ? ` · ${date(s.board.bios_date)}` : ""}</dd>
            <dt>Form factor</dt>
            <dd>{s.is_laptop ? "Laptop" : "Desktop"}{s.has_battery ? " · battery present" : ""}</dd>
          </dl>
        </Panel>
      </div>

      <Panel title="Processor" sub={s.cpu.name}>
        <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1.4fr)", alignItems: "start" }}>
          <dl className="kv">
            <dt>Cores / threads</dt>
            <dd>{or(s.cpu.cores)} / {or(s.cpu.threads)}</dd>
            <dt>Max clock</dt>
            <dd>{s.cpu.max_clock_mhz ? `${(s.cpu.max_clock_mhz / 1000).toFixed(2)} GHz` : "—"}</dd>
            <dt>Current clock</dt>
            <dd>{latest ? `${(latest.cpu_freq_mhz / 1000).toFixed(2)} GHz` : "—"}</dd>
            <dt>Cache</dt>
            <dd>{[s.cpu.l2_kb && `L2 ${bytes(s.cpu.l2_kb * 1024, 0)}`, s.cpu.l3_kb && `L3 ${bytes(s.cpu.l3_kb * 1024, 0)}`].filter(Boolean).join(" · ") || "—"}</dd>
            <dt>Virtualisation</dt>
            <dd>{s.cpu.virtualization_enabled == null ? "—" : s.cpu.virtualization_enabled ? "Enabled" : "Disabled in firmware"}</dd>
            <dt>Temperature</dt>
            <dd className="muted">Not exposed by Windows without vendor drivers</dd>
          </dl>
          <div>
            <div className="small muted" style={{ marginBottom: 8 }}>Load per logical processor (live)</div>
            <div className="cores" role="img" aria-label="Per-core CPU load">
              {(latest?.cpu_per_core ?? []).map((v, i) => (
                <span key={i} style={{ height: `${Math.max(3, v)}%` }} title={`CPU ${i}: ${v.toFixed(0)}%`} />
              ))}
            </div>
          </div>
        </div>
      </Panel>

      <div className="grid cols-2">
        <Panel title="Graphics">
          {s.gpus.length === 0 && <Empty title="No display adapters reported" />}
          <div className="list">
            {s.gpus.map((g, i) => {
              const live = latest?.gpus.find((x) => x.name === g.name);
              return (
                <div key={i} className="list-item">
                  <div className="body">
                    <div className="title">{g.name}</div>
                    <dl className="kv mt-1">
                      <dt>Driver</dt>
                      <dd>{or(g.driver_version)}{g.driver_date ? ` · ${date(g.driver_date)}` : ""}</dd>
                      <dt>Video memory</dt>
                      <dd>{g.vram_bytes ? bytes(g.vram_bytes) : live?.vram_total ? bytes(live.vram_total) : "—"}</dd>
                      <dt>Display</dt>
                      <dd>{[g.resolution, g.refresh_hz && `${g.refresh_hz} Hz`].filter(Boolean).join(" · ") || "—"}</dd>
                      <dt>Usage · temp</dt>
                      <dd>
                        {live?.usage != null ? `${live.usage.toFixed(0)}%` : "—"} · {live?.temperature_c != null ? `${live.temperature_c.toFixed(0)} °C` : "temperature not exposed"}
                      </dd>
                    </dl>
                  </div>
                </div>
              );
            })}
          </div>
        </Panel>
        <Panel title="Memory" sub={s.memory_total_bytes ? `${bytes(s.memory_total_bytes)} installed` : undefined}>
          {s.memory_modules.length === 0 ? (
            <div className="small muted">Module details weren't reported by firmware.</div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Slot</th>
                  <th className="r">Size</th>
                  <th className="r">Speed</th>
                  <th>Part</th>
                </tr>
              </thead>
              <tbody>
                {s.memory_modules.map((m, i) => (
                  <tr key={i}>
                    <td className="small">{or(m.slot)}</td>
                    <td className="r num">{bytes(m.capacity_bytes, 0)}</td>
                    <td className="r num small">
                      {m.configured_mts ?? "—"}
                      {m.speed_mts && m.configured_mts && m.speed_mts > m.configured_mts ? <span className="dim"> / {m.speed_mts} rated</span> : ""} MT/s
                    </td>
                    <td className="small dim truncate" style={{ maxWidth: 160 }}>{[m.manufacturer, m.part_number?.trim()].filter(Boolean).join(" ") || "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {s.memory_modules.some((m) => m.speed_mts && m.configured_mts && m.speed_mts > m.configured_mts) && (
            <div className="small muted mt-1">Memory is running below its rated speed. Enabling XMP/EXPO in the BIOS can fix this — NOCTURN can't change firmware settings.</div>
          )}
        </Panel>
      </div>

      <Panel title="Storage devices">
        {s.disks.length === 0 ? (
          <div className="small muted">No physical disks reported.</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Model</th>
                <th>Type</th>
                <th>Bus</th>
                <th className="r">Size</th>
                <th>Health</th>
              </tr>
            </thead>
            <tbody>
              {s.disks.map((d, i) => (
                <tr key={i}>
                  <td>{d.model}</td>
                  <td className="small">{or(d.media_type)}</td>
                  <td className="small">{or(d.bus_type)}</td>
                  <td className="r num">{bytes(d.size_bytes)}</td>
                  <td>
                    <span className={`tag ${d.health === "Healthy" ? "good" : d.health ? "warning" : ""}`}>{d.health ?? "Unknown"}</span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>
    </div>
  );
}

// ------------------------------------------------------------------ events

const LEVEL: Record<number, { label: string; tone: string }> = {
  1: { label: "Critical", tone: "critical" },
  2: { label: "Error", tone: "serious" },
  3: { label: "Warning", tone: "warning" },
  4: { label: "Info", tone: "" },
};

function Events() {
  const events = useLoad(() => api.events(), []);
  const [open, setOpen] = useState<number | null>(null);
  const [level, setLevel] = useState<"all" | "critical">("all");
  const rows = useMemo(() => (events.data ?? []).filter((e) => level === "all" || e.level <= 1), [events.data, level]);
  const grouped = useMemo(() => {
    const m = new Map<string, { e: EventEntry; n: number }>();
    for (const e of rows) {
      const k = `${e.provider}|${e.id}`;
      const g = m.get(k);
      if (g) g.n++;
      else m.set(k, { e, n: 1 });
    }
    return [...m.values()];
  }, [rows]);

  if (events.loading && !events.data) return <Loading label="Reading System and Application logs…" />;
  if (events.error) return <ErrorNote message={events.error} onRetry={events.reload} />;
  return (
    <section className="panel flush">
      <div className="panel-head" style={{ paddingBottom: 12 }}>
        <div>
          <h2>Recent errors and warnings</h2>
          <div className="sub">Grouped by source and event ID. Most errors in the event log are harmless — repeated or critical ones are worth a look.</div>
        </div>
        <div className="row">
          <Seg value={level} onChange={setLevel} options={[{ value: "all", label: "All" }, { value: "critical", label: "Critical only" }]} />
          <button className="btn sm icon" onClick={events.reload} aria-label="Refresh"><I.refresh /></button>
        </div>
      </div>
      {grouped.length === 0 ? (
        <Empty title="Nothing to show">No matching errors in the recent event log.</Empty>
      ) : (
        <div className="table-wrap" style={{ maxHeight: "calc(100vh - 300px)" }}>
          <table className="table">
            <thead>
              <tr>
                <th style={{ width: 90 }}>Level</th>
                <th>Source</th>
                <th className="r" style={{ width: 70 }}>Count</th>
                <th style={{ width: 150 }}>Latest</th>
              </tr>
            </thead>
            <tbody>
              {grouped.map(({ e, n }, i) => (
                <tr key={i} onClick={() => setOpen(open === i ? null : i)} style={{ cursor: "default" }}>
                  <td>
                    <span className={`tag ${LEVEL[e.level]?.tone ?? ""}`}>{LEVEL[e.level]?.label ?? `Level ${e.level}`}</span>
                  </td>
                  <td style={{ maxWidth: 0 }}>
                    <div>
                      {e.provider} <span className="dim">· ID {e.id} · {e.log}</span>
                    </div>
                    <div className={`small muted selectable ${open === i ? "" : "truncate"}`} style={{ whiteSpace: open === i ? "pre-wrap" : undefined }}>
                      {e.message || "(no message)"}
                    </div>
                  </td>
                  <td className="r num">{n}</td>
                  <td className="small muted">{relative(e.time)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

// ----------------------------------------------------------------- drivers

function Drivers() {
  const drivers = useLoad(() => api.drivers(), []);
  const [q, setQ] = useState("");
  if (drivers.loading && !drivers.data) return <Loading label="Enumerating signed drivers…" />;
  if (drivers.error) return <ErrorNote message={drivers.error} onRetry={drivers.reload} />;
  const now = Date.now();
  const rows = (drivers.data ?? [])
    .filter((d) => !q || `${d.device} ${d.provider} ${d.class}`.toLowerCase().includes(q.toLowerCase()))
    .sort((a, b) => (a.class ?? "").localeCompare(b.class ?? "") || a.device.localeCompare(b.device));
  const unsigned = (drivers.data ?? []).filter((d) => d.signed === false).length;
  return (
    <section className="panel flush">
      <div className="panel-head" style={{ paddingBottom: 12 }}>
        <div>
          <h2>All maker drivers</h2>
          <div className="sub">
            {drivers.data?.length ?? 0} drivers{unsigned ? ` · ${unsigned} unsigned` : ""}. Update graphics, chipset and network drivers from the manufacturer's site, not third-party updaters.
          </div>
        </div>
        <input className="input" style={{ width: 240, height: 30 }} placeholder="Filter device, vendor, class" value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <div className="table-wrap" style={{ maxHeight: "calc(100vh - 300px)" }}>
        <table className="table">
          <thead>
            <tr>
              <th>Device</th>
              <th>Class</th>
              <th>Provider</th>
              <th>Version</th>
              <th>Date</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((d, i) => {
              const old = d.date ? now - new Date(d.date).getTime() > 1000 * 86400 * 365 * 3 : false;
              return (
                <tr key={i}>
                  <td>
                    {d.device} {d.signed === false && <span className="tag warning">Unsigned</span>}
                  </td>
                  <td className="small muted">{d.class ?? "—"}</td>
                  <td className="small">{d.provider ?? "—"}</td>
                  <td className="mono small">{d.version ?? "—"}</td>
                  <td className="small" style={{ color: old ? "#f6cc6e" : undefined }} title={old ? "Older than three years" : undefined}>
                    {date(d.date)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------- services

// Services commonly asked about, with the honest recommendation. NOCTURN does
// not change service configuration; these are explanations only.
const SERVICE_NOTES: Record<string, string> = {
  sysmain: "Preloads frequently used apps. Leave on for SSDs and HDDs alike; disabling rarely helps.",
  wsearch: "Windows Search indexing. Leave on unless you never search; high disk use usually settles after indexing finishes.",
  diagtrack: "Connected User Experiences and Telemetry. Telemetry level is controlled under Tweak › Privacy rather than by disabling the service.",
  spooler: "Print Spooler. Needed for printing; safe to leave running.",
  wuauserv: "Windows Update. Keep enabled — security updates depend on it.",
  windefend: "Microsoft Defender. Keep enabled.",
  mpssvc: "Windows Firewall. Keep enabled.",
  xblgamesave: "Xbox game save sync. Harmless when idle; only runs when needed.",
};

function Services() {
  const { navigate } = useApp();
  const services = useLoad(() => api.services(), []);
  const [q, setQ] = useState("");
  const [running, setRunning] = useState(true);
  if (services.loading && !services.data) return <Loading label="Reading services…" />;
  if (services.error) return <ErrorNote message={services.error} onRetry={services.reload} />;
  const rows = (services.data ?? [])
    .filter((s) => !running || s.state.toLowerCase() === "running")
    .filter((s) => !q || `${s.name} ${s.display_name}`.toLowerCase().includes(q.toLowerCase()))
    .sort((a, b) => a.display_name.localeCompare(b.display_name));
  return (
    <div className="stack" style={{ gap: 14 }}>
      <Note>
        A quick read-only list. To change how a service starts, use the <a href="#" onClick={(e) => { e.preventDefault(); navigate("services"); }}>Services</a> page, which explains each optional service and protects security and core ones. Disabling services is rarely a worthwhile speed-up on modern PCs.
      </Note>
      <section className="panel flush">
        <div className="panel-head" style={{ paddingBottom: 12 }}>
          <div>
            <h2>Services</h2>
            <div className="sub">{rows.length} shown</div>
          </div>
          <div className="row">
            <label className="row small muted" style={{ gap: 8 }}>
              <input type="checkbox" className="check" checked={running} onChange={(e) => setRunning(e.target.checked)} /> Running only
            </label>
            <input className="input" style={{ width: 220, height: 30 }} placeholder="Filter services" value={q} onChange={(e) => setQ(e.target.value)} />
          </div>
        </div>
        <div className="table-wrap" style={{ maxHeight: "calc(100vh - 340px)" }}>
          <table className="table">
            <thead>
              <tr>
                <th>Service</th>
                <th style={{ width: 110 }}>Start</th>
                <th style={{ width: 100 }}>State</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((s) => (
                <tr key={s.name}>
                  <td>
                    <div>
                      {s.display_name} <span className="mono dim">{s.name}</span>
                    </div>
                    {SERVICE_NOTES[s.name.toLowerCase()] && <div className="small muted">{SERVICE_NOTES[s.name.toLowerCase()]}</div>}
                  </td>
                  <td className="small">{s.start_mode}</td>
                  <td className="small">
                    <span className="row" style={{ gap: 6 }}>
                      <span className="status-dot" style={{ background: s.state.toLowerCase() === "running" ? "var(--good)" : "var(--ink-3)" }} />
                      {s.state}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

// ----------------------------------------------------------------- restore

function Restore() {
  const r = useLoad(() => api.restoreStatus(), []);
  if (r.loading && !r.data) return <Loading label="Checking System Restore…" />;
  if (r.error) return <ErrorNote message={r.error} onRetry={r.reload} />;
  const pts = [...(r.data?.points ?? [])].sort((a, b) => (b.SequenceNumber ?? 0) - (a.SequenceNumber ?? 0));
  return (
    <div className="stack" style={{ gap: 14 }}>
      {!r.data?.queryable && (
        <Note tone="warning">
          Restore points can only be listed with administrator rights, or System Restore is turned off. NOCTURN still records the original value of every change it makes, so rollback from History works either way.
        </Note>
      )}
      <Panel title="Windows restore points" sub="NOCTURN offers to create one before changes that need administrator rights">
        {pts.length === 0 ? (
          <Empty title={r.data?.queryable ? "No restore points" : "Not available"}>
            {r.data?.queryable ? "System Restore is on but no points exist yet." : "Run NOCTURN as administrator to list them."}
          </Empty>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Description</th>
                <th style={{ width: 200 }}>Created</th>
                <th className="r" style={{ width: 80 }}>#</th>
              </tr>
            </thead>
            <tbody>
              {pts.map((p, i) => (
                <tr key={i}>
                  <td>{p.Description ?? "—"}</td>
                  <td className="small muted">{wmiDate(p.Created)}</td>
                  <td className="r num dim">{p.SequenceNumber ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <div className="small dim mt-2">To restore Windows to one of these points, run <span className="mono">rstrui.exe</span> from the Start menu.</div>
      </Panel>
    </div>
  );
}

/** WMI CIM_DATETIME (yyyymmddHHMMSS.ffffff+UUU) or ISO → readable date. */
function wmiDate(s?: string) {
  if (!s) return "—";
  const m = /^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})/.exec(s);
  if (m) return date(`${m[1]}-${m[2]}-${m[3]}T${m[4]}:${m[5]}:${m[6]}`, true);
  return date(s, true);
}
