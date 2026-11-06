// Driver centre (System Diagnostics › Drivers): problem devices explained,
// the drivers worth keeping current, recent installs, official support links.
// NOCTURN never downloads or installs drivers itself.

import { api, errorMessage, type SupportLink } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { date, relative } from "../lib/format";
import { I } from "./Icons";
import { Empty, ErrorNote, Loading, Panel } from "./ui";

function LinkButton({ link }: { link: SupportLink | null }) {
  const { toast } = useApp();
  if (!link) return null;
  return (
    <button
      className="btn ghost sm"
      title={`Opens ${link.label}`}
      onClick={() => api.openSupportLink(link.key).catch((e) => toast({ tone: "critical", title: "Couldn't open the page", detail: errorMessage(e) }))}
    >
      {link.label} <I.arrow />
    </button>
  );
}

const SEV: Record<string, string> = { critical: "critical", warning: "warning", info: "" };

export function DriverCentre() {
  const { navigate } = useApp();
  const rep = useLoad(() => api.driverCentre(), []);
  if (rep.loading && !rep.data) return <Loading label="Checking devices and drivers…" />;
  if (rep.error || !rep.data) return <ErrorNote message={rep.error ?? "No data"} onRetry={rep.reload} />;
  const r = rep.data;

  return (
    <div className="stack" style={{ gap: 14 }}>
      <Panel
        title="Devices with problems"
        sub="From Device Manager. Disconnected devices Windows still remembers are left out."
        actions={<button className="btn ghost sm icon" onClick={rep.reload} aria-label="Refresh"><I.refresh /></button>}
      >
        {r.problems.length === 0 ? (
          <Empty title="Every connected device is working" />
        ) : (
          <div className="list">
            {r.problems.map((p) => (
              <div key={p.id} className="list-item">
                <div className="body">
                  <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                    <span className={`tag ${SEV[p.severity]}`}>Code {p.code}</span>
                    <span className="title">{p.name}</span>
                    {p.identified && <span className="small dim" title="Windows has no name for it; NOCTURN identified it from its hardware ID">identified from hardware ID</span>}
                  </div>
                  <div className="desc">{p.meaning} {p.action}</div>
                  <div className="mono dim small selectable truncate" title={p.id}>{p.id}</div>
                </div>
                <div className="row" style={{ gap: 6 }}>
                  <LinkButton link={p.link} />
                  <button className="btn ghost sm" onClick={() => navigate("assistant", { prompt: `Device Manager shows "${p.name}" with error code ${p.code} (${p.meaning}). Device ID: ${p.id}. How do I fix it safely?` })}>
                    <I.assistant /> Ask
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </Panel>

      <Panel
        title="Drivers worth keeping current"
        sub="Graphics, network, audio, Bluetooth, chipset and storage. Get updates from the maker's official site — avoid third-party driver updaters."
        actions={<div className="row" style={{ gap: 6 }}><LinkButton link={r.board_link} /><LinkButton link={{ key: "windows_update", label: "Optional updates" }} /></div>}
      >
        {r.key.length === 0 ? (
          <Empty title="No drivers found in these groups" />
        ) : (
          <table className="table">
            <thead><tr><th style={{ width: 90 }}>Group</th><th>Device</th><th>Provider</th><th>Version</th><th style={{ width: 110 }}>Date</th><th className="r" /></tr></thead>
            <tbody>
              {r.key.map((k, i) => (
                <tr key={i}>
                  <td className="small muted">{k.group}</td>
                  <td>
                    <div className="row" style={{ gap: 8 }}>
                      <span>{k.device}</span>
                      {k.signed === false && <span className="tag warning">Unsigned</span>}
                    </div>
                    {k.note && <div className="small" style={{ color: k.built_in ? "var(--ink-2)" : "var(--ink-warning)" }}>{k.note}</div>}
                  </td>
                  <td className="small">{k.provider ?? "—"}</td>
                  <td className="mono small">{k.version ?? "—"}</td>
                  <td className="small">{k.built_in ? <span className="dim">Windows</span> : date(k.date)}</td>
                  <td className="r"><LinkButton link={k.link} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>

      <Panel title="Driver installs, last 90 days" sub="Drivers from makers (not Windows' built-in ones) installed or updated, from Windows' device configuration log.">
        {r.installs.length === 0 ? (
          <Empty title="No maker drivers installed in the last 90 days" />
        ) : (
          <div className="table-wrap" style={{ maxHeight: 320 }}>
            <table className="table">
              <thead><tr><th style={{ width: 120 }}>When</th><th>Device</th><th>Provider</th><th>Version</th><th style={{ width: 90 }} /></tr></thead>
              <tbody>
                {r.installs.map((x, i) => (
                  <tr key={i}>
                    <td className="small" title={date(x.time, true)}>{relative(x.time)}</td>
                    <td>{x.device}</td>
                    <td className="small">{x.provider ?? "—"}</td>
                    <td className="mono small">{x.version ?? "—"}</td>
                    <td className="small dim">{x.updated ? "update" : "new install"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
    </div>
  );
}
