// Crash analyser tab (System Diagnostics): findings, a 90-day incident
// timeline, Reliability Monitor's stability index, system incidents and
// crashing apps — each explained, with the evidence it came from.

import { useMemo, useRef, useState } from "react";
import { api, type CrashDay, type CrashFinding, type CrashReport } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { bytes, date, relative } from "../lib/format";
import { I } from "./Icons";
import { LineChart } from "./LineChart";
import { Empty, ErrorNote, Loading, Panel } from "./ui";

const KIND_LABEL = { blue_screen: "Blue screen", unexpected_shutdown: "Unexpected shutdown", hardware_error: "Hardware error" } as const;
const SEVERITY_TAG: Record<CrashFinding["severity"], { cls: string; label: string }> = {
  good: { cls: "good", label: "Healthy" },
  info: { cls: "", label: "Note" },
  warning: { cls: "warning", label: "Worth checking" },
  critical: { cls: "critical", label: "Act on this" },
};

// ---------------------------------------------------------------- timeline

const SYS = "var(--series-1)";
const APP = "var(--series-2)";

function Timeline({ days }: { days: CrashDay[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [table, setTable] = useState(false);
  const rows = days.map((d) => ({ day: d.day, sys: d.blue_screens + d.shutdowns, app: d.app_crashes + d.hangs, d }));
  const max = Math.max(1, ...rows.map((r) => r.sys + r.app));
  const W = 900;
  const H = 110;
  const pad = { l: 22, b: 18, t: 6 };
  const slot = (W - pad.l) / rows.length;
  const bw = Math.max(2, slot - 2);
  const y = (v: number) => ((H - pad.b - pad.t) * v) / max;
  const busy = rows.filter((r) => r.sys + r.app > 0);
  const hv = hover != null ? rows[hover] : null;

  // Rounded top on the topmost segment only; square at the baseline.
  const bar = (x: number, top: number, h: number, round: boolean) => {
    const r = round ? Math.min(4, bw / 2, h) : 0;
    return `M${x},${top + h} V${top + r} Q${x},${top} ${x + r},${top} H${x + bw - r} Q${x + bw},${top} ${x + bw},${top + r} V${top + h} Z`;
  };

  return (
    <div>
      <div className="row between" style={{ marginBottom: 6 }}>
        <div className="legend row" style={{ gap: 14 }}>
          <span className="item small"><span className="swatch" style={{ background: SYS }} /> System crashes &amp; shutdowns</span>
          <span className="item small"><span className="swatch" style={{ background: APP }} /> App crashes &amp; hangs</span>
        </div>
        <button className="btn ghost sm" onClick={() => setTable((t) => !t)}>{table ? "Show chart" : "Show as table"}</button>
      </div>
      {table ? (
        busy.length === 0 ? (
          <div className="small muted">No incidents on any day.</div>
        ) : (
          <table className="table">
            <thead><tr><th>Day</th><th className="r">Blue screens</th><th className="r">Shutdowns</th><th className="r">App crashes</th><th className="r">Hangs</th></tr></thead>
            <tbody>
              {busy.map((r) => (
                <tr key={r.day}><td>{date(r.day)}</td><td className="r num">{r.d.blue_screens}</td><td className="r num">{r.d.shutdowns}</td><td className="r num">{r.d.app_crashes}</td><td className="r num">{r.d.hangs}</td></tr>
              ))}
            </tbody>
          </table>
        )
      ) : (
        <div className="chart" ref={ref} onMouseLeave={() => setHover(null)}>
          <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Incidents per day over ${rows.length} days: ${busy.length} days with incidents`}>
            {[0, max].map((v) => (
              <g key={v}>
                <line x1={pad.l} x2={W} y1={H - pad.b - y(v)} y2={H - pad.b - y(v)} stroke="var(--grid)" strokeWidth={1} />
                <text x={pad.l - 6} y={H - pad.b - y(v) + 4} textAnchor="end" fontSize={10} fill="var(--ink-3)">{v}</text>
              </g>
            ))}
            {rows.map((r, i) => {
              const x = pad.l + i * slot + 1;
              const base = H - pad.b;
              const hs = y(r.sys);
              const ha = y(r.app);
              const gap = hs > 0 && ha > 0 ? 2 : 0;
              return (
                <g key={r.day}>
                  {hs > 0 && <path d={bar(x, base - hs, hs, ha === 0)} fill={SYS} opacity={hover == null || hover === i ? 1 : 0.45} />}
                  {ha > 0 && <path d={bar(x, base - hs - gap - ha, ha, true)} fill={APP} opacity={hover == null || hover === i ? 1 : 0.45} />}
                  {/* hit target: the whole column, taller than the mark */}
                  <rect x={pad.l + i * slot} y={0} width={slot} height={H - pad.b} fill="transparent" onMouseEnter={() => setHover(i)} />
                </g>
              );
            })}
            {[0, Math.floor(rows.length / 2), rows.length - 1].map((i) => (
              <text key={i} x={pad.l + i * slot + slot / 2} y={H - 4} textAnchor={i === 0 ? "start" : i === rows.length - 1 ? "end" : "middle"} fontSize={10} fill="var(--ink-3)">
                {date(rows[i]?.day)}
              </text>
            ))}
          </svg>
          {hv && hover != null && (
            <div className="tip" style={{ left: `${Math.min(80, Math.max(2, ((pad.l + hover * slot) / W) * 100))}%`, top: 0 }}>
              <div className="t">{date(hv.day)}</div>
              <div className="r"><b>{hv.sys}</b><i style={{ background: SYS }} /><span>system ({hv.d.blue_screens} blue screen, {hv.d.shutdowns} shutdown)</span></div>
              <div className="r"><b>{hv.app}</b><i style={{ background: APP }} /><span>apps ({hv.d.app_crashes} crash, {hv.d.hangs} hang)</span></div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// -------------------------------------------------------------------- tab

function askAbout(r: CrashReport, what: string) {
  return `Help me understand this crash on my PC and what to do about it, step by step. ${what} (From NOCTURN's crash analyser, last ${r.window_days} days: ${r.blue_screens} blue screens, ${r.unexpected_shutdowns} unexpected shutdowns, ${r.app_crashes} app crashes.)`;
}

export function Crashes() {
  const { navigate } = useApp();
  const rep = useLoad(() => api.crashReport(), []);
  const [openApp, setOpenApp] = useState<string | null>(null);
  const r = rep.data;
  const stability = useMemo(() => (r?.stability ?? []).map(([d, v]) => ({ t: Date.parse(d + "T12:00:00Z"), v })), [r]);

  if (rep.loading && !r) return <Loading label="Reading crash records from the event log…" />;
  if (rep.error || !r) return <ErrorNote message={rep.error ?? "No data"} onRetry={rep.reload} />;

  const stab = stability.length ? stability[stability.length - 1].v : null;
  return (
    <div className="stack" style={{ gap: 14 }}>
      <div className="grid cols-4">
        <div className="panel stat">
          <div className="label">Blue screens</div>
          <div className="value num">{r.blue_screens + r.hardware_errors}</div>
          <div className="foot">last {r.window_days} days{r.hardware_errors ? ` · ${r.hardware_errors} hardware` : ""}</div>
        </div>
        <div className="panel stat">
          <div className="label">Unexpected shutdowns</div>
          <div className="value num">{r.unexpected_shutdowns}</div>
          <div className="foot">{r.last_system_crash ? `last ${relative(r.last_system_crash)}` : "none recorded"}</div>
        </div>
        <div className="panel stat">
          <div className="label">App crashes</div>
          <div className="value num">{r.app_crashes}</div>
          <div className="foot">{r.app_hangs} stopped responding · {r.apps.length} apps</div>
        </div>
        <div className="panel stat">
          <div className="label">Stability index</div>
          <div className="value num">{stab != null ? stab.toFixed(1) : "—"}<span className="small dim"> / 10</span></div>
          <div className="foot">{stab != null ? "Reliability Monitor's score" : "Reliability Monitor has no data"}</div>
        </div>
      </div>

      <Panel title="What NOCTURN found" sub="Each conclusion is drawn from the records listed below." actions={<button className="btn ghost sm icon" onClick={rep.reload} aria-label="Refresh"><I.refresh /></button>}>
        <div className="list">
          {r.findings.map((f) => (
            <div key={f.title} className="list-item">
              <div className="body">
                <div className="row" style={{ gap: 8 }}>
                  <span className={`tag ${SEVERITY_TAG[f.severity].cls}`}>{f.severity === "good" && <I.check />} {SEVERITY_TAG[f.severity].label}</span>
                  <span className="title">{f.title}</span>
                </div>
                <div className="desc">{f.detail}</div>
                {f.steps.length > 0 && (
                  <ol className="small" style={{ margin: "6px 0 0", paddingLeft: 18 }}>
                    {f.steps.map((s) => <li key={s}>{s}</li>)}
                  </ol>
                )}
              </div>
              {f.severity !== "good" && (
                <button className="btn ghost sm" onClick={() => navigate("assistant", { prompt: askAbout(r, `${f.title}. ${f.detail}`) })}><I.assistant /> Ask</button>
              )}
            </div>
          ))}
        </div>
      </Panel>

      <Panel title={`Incidents per day, last ${r.window_days} days`}>
        <Timeline days={r.daily} />
      </Panel>

      {stability.length > 1 && (
        <Panel title="Stability index" sub="Windows Reliability Monitor's daily score from 1 (unstable) to 10 (stable). Crashes and failed updates lower it; it recovers over trouble-free days.">
          <LineChart
            ariaLabel="Stability index per day"
            series={[{ key: "s", label: "Stability index", color: "var(--series-1)", values: stability.map((p) => p.v) }]}
            times={stability.map((p) => p.t)}
            max={10}
            format={(v) => v.toFixed(1)}
            height={90}
            timeLabel={(t) => date(new Date(t).toISOString())}
          />
        </Panel>
      )}

      <Panel title="System incidents" sub="Blue screens, unexpected shutdowns and fatal hardware errors. One incident often leaves several event-log records; they're merged.">
        {r.incidents.length === 0 ? (
          <Empty title={`None in the last ${r.window_days} days`} />
        ) : (
          <table className="table">
            <thead><tr><th style={{ width: 150 }}>When</th><th>What happened</th><th style={{ width: 200 }}>Likely area</th></tr></thead>
            <tbody>
              {r.incidents.map((i) => (
                <tr key={i.time + i.kind}>
                  <td className="small">{date(i.time, true)}</td>
                  <td>
                    <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                      <span style={{ fontWeight: 500 }}>{KIND_LABEL[i.kind]}</span>
                      {i.code && <span className="mono small">{i.code}</span>}
                      {i.code_name && <span className="tag">{i.code_name}</span>}
                    </div>
                    <div className="small muted">{i.explanation}</div>
                    <div className="small dim">From: {i.evidence.join(", ")}{i.dump ? ` · dump: ${i.dump}` : ""}</div>
                  </td>
                  <td className="small">{i.area_label}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>

      <Panel title="Apps that crashed or stopped responding" sub="Grouped by app. The faulting module shows where the error happened, which is not always what caused it.">
        {r.apps.length === 0 ? (
          <Empty title={`No app crashes in the last ${r.window_days} days`} />
        ) : (
          <table className="table">
            <thead><tr><th>App</th><th className="r" style={{ width: 80 }}>Crashes</th><th className="r" style={{ width: 80 }}>Hangs</th><th style={{ width: 120 }}>Last</th><th className="r" style={{ width: 80 }} /></tr></thead>
            <tbody>
              {r.apps.map((a) => {
                const open = openApp === a.app;
                return (
                  <tr key={a.app} onClick={() => setOpenApp(open ? null : a.app)} style={{ cursor: "default" }}>
                    <td style={{ maxWidth: 0 }}>
                      <div style={{ fontWeight: 500 }}>{a.app}{a.version && <span className="dim small"> {a.version}</span>}</div>
                      <div className="small muted">{a.area_label}: {a.explanation}</div>
                      {open && (
                        <div className="stack small" style={{ gap: 4, marginTop: 6 }}>
                          {a.path && <div className="mono dim selectable truncate" title={a.path}>{a.path}</div>}
                          {a.modules.length > 0 && <div><span className="dim">Faulting module: </span>{a.modules.map((m) => `${m.name} ×${m.count}`).join(", ")}</div>}
                          {a.codes.map((c) => (
                            <div key={c.name}><span className="mono">{c.name}</span> ×{c.count}{c.detail && <span className="muted"> — {c.detail}</span>}</div>
                          ))}
                        </div>
                      )}
                    </td>
                    <td className="r num">{a.crashes}</td>
                    <td className="r num">{a.hangs}</td>
                    <td className="small">{relative(a.last)}</td>
                    <td className="r">
                      <button
                        className="btn ghost sm"
                        onClick={(e) => {
                          e.stopPropagation();
                          navigate("assistant", { prompt: askAbout(r, `${a.app} crashed ${a.crashes} times and hung ${a.hangs} times. Faulting module: ${a.modules.map((m) => m.name).join(", ") || "unknown"}. Errors: ${a.codes.map((c) => c.name).join(", ") || "none recorded"}.`) });
                        }}
                      >
                        <I.assistant /> Ask
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </Panel>

      <div className="small dim">
        Crash dumps:{" "}
        {r.dumps.minidumps === null
          ? "the Minidump folder needs administrator rights to read."
          : r.dumps.minidumps.length === 0 && !r.dumps.memory_dmp
            ? "none on this PC (Windows deletes them after reporting, or dumps are turned off)."
            : [r.dumps.minidumps.length ? `${r.dumps.minidumps.length} minidump(s), latest ${relative(r.dumps.minidumps[0].modified)}` : null, r.dumps.memory_dmp ? `MEMORY.DMP ${bytes(r.dumps.memory_dmp.size)}` : null].filter(Boolean).join(" · ")}
        {" "}Stop codes above come from the event log, so they're available even without dumps.
      </div>
    </div>
  );
}
