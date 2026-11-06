// Network: adapters, DNS and Wi-Fi details, a measured stability test, and
// the network optimisations from the catalogue.

import { useState } from "react";
import { LineChart } from "../components/LineChart";
import { DnsPanel, TestHistory, TraceroutePanel } from "../components/NetworkTools";
import { I } from "../components/Icons";
import { TweakList } from "../components/TweakList";
import { Empty, ErrorNote, Loading, Meter, Note, PageHead, Panel, Spinner } from "../components/ui";
import { api, errorMessage, type Adapter, type StabilityReport } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { rate, relative } from "../lib/format";
import { useTelemetry } from "../lib/telemetry";

export function Network() {
  const { dataVersion, navigate } = useApp();
  const { history } = useTelemetry();
  const net = useLoad(() => api.networkOverview(), []);
  const tweaks = useLoad(() => api.tweaks(), [dataVersion]);
  const [test, setTest] = useState<StabilityReport | null>(null);
  const [testing, setTesting] = useState(false);
  const [testError, setTestError] = useState<string | null>(null);
  const [showVirtual, setShowVirtual] = useState(false);
  const [historyVersion, setHistoryVersion] = useState(0);

  async function runTest() {
    setTesting(true);
    setTestError(null);
    try {
      setTest(await api.networkTest(20));
      setHistoryVersion((v) => v + 1);
    } catch (e) {
      setTestError(errorMessage(e));
    } finally {
      setTesting(false);
    }
  }

  const adapters = (net.data?.adapters ?? []).filter((a) => showVirtual || !a.is_virtual);
  const hidden = (net.data?.adapters ?? []).filter((a) => a.is_virtual).length;
  const wifi = net.data?.wifi;

  return (
    <div className="page">
      <PageHead
        eyebrow="Network"
        title="Network diagnostics"
        desc="Adapter, DNS and Wi-Fi details read from Windows, a stability test with history, traceroute, and DNS provider switching you can undo."
        actions={
          <>
            <button className="btn" onClick={() => navigate("assistant", { prompt: "My PC randomly disconnects or has unstable internet. Please run network diagnostics and tell me what's wrong." })}>
              <I.assistant /> Ask AI
            </button>
            <button className="btn primary" onClick={runTest} disabled={testing}>
              {testing ? <Spinner /> : <I.bolt />} {testing ? "Testing… (~20 s)" : "Run stability test"}
            </button>
          </>
        }
      />

      <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1.4fr) minmax(0, 1fr)" }}>
        <Panel title="Throughput" sub="All adapters, last two minutes">
          <LineChart
            ariaLabel="Network throughput, last two minutes"
            series={[
              { key: "rx", label: "Download", color: "var(--series-1)", values: history.map((s) => s.net_rx_bps) },
              { key: "tx", label: "Upload", color: "var(--series-2)", values: history.map((s) => s.net_tx_bps) },
            ]}
            times={history.map((s) => s.ts)}
            format={rate}
            height={150}
            ticks={3}
          />
        </Panel>
        <Panel title="Wi-Fi" sub={!net.data ? "Checking…" : wifi ? (wifi.connected ? wifi.ssid ?? "Connected" : "Not connected") : "No wireless adapter in use"}>
          {net.loading && !net.data && <Loading />}
          {wifi && wifi.connected ? (
            <div className="stack" style={{ gap: 12 }}>
              {wifi.signal_percent != null && (
                <div className="stack" style={{ gap: 6 }}>
                  <div className="row between small">
                    <span>Signal</span>
                    <span className="num">{wifi.signal_percent}%{wifi.signal_percent < 50 ? " · weak" : ""}</span>
                  </div>
                  <Meter value={100 - wifi.signal_percent} warnAt={50} critAt={70} label="Signal weakness" />
                </div>
              )}
              <dl className="kv">
                <dt>Standard</dt>
                <dd>{wifi.radio ?? "—"}</dd>
                <dt>Band · channel</dt>
                <dd>{[wifi.band, wifi.channel].filter(Boolean).join(" · ") || "—"}</dd>
                <dt>Link rate</dt>
                <dd>{wifi.receive_mbps != null ? `↓ ${wifi.receive_mbps} Mb/s · ↑ ${wifi.transmit_mbps ?? "—"} Mb/s` : "—"}</dd>
              </dl>
              {wifi.signal_percent != null && wifi.signal_percent < 50 && (
                <Note tone="warning">Weak signal often causes drops and lag spikes. Moving closer to the router, using 5 GHz, or a wired connection usually helps more than any software setting.</Note>
              )}
            </div>
          ) : (
            net.data && <div className="small muted">{wifi ? "Wi-Fi is not connected." : "This PC isn't using Wi-Fi right now."}</div>
          )}
        </Panel>
      </div>

      {(test || testing || testError) && (
        <Panel className="mt-2" title="Connection stability" sub={test ? `Tested ${relative(test.tested_at)} · 20 pings per target` : "Pinging your router and public resolvers…"}>
          {testError && <ErrorNote message={testError} onRetry={runTest} />}
          {testing && !test && <Loading label="Measuring latency and packet loss…" />}
          {test && (
            <div className="grid" style={{ gridTemplateColumns: "minmax(0, 2fr) minmax(0, 1fr)" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>Target</th>
                    <th className="r">Avg</th>
                    <th className="r">Min / Max</th>
                    <th className="r">Jitter</th>
                    <th className="r">Loss</th>
                  </tr>
                </thead>
                <tbody>
                  {test.pings.map((p) => (
                    <tr key={p.target}>
                      <td>
                        {p.label} <span className="mono dim">{p.target}</span>
                      </td>
                      <td className="r num">{p.avg_ms != null ? `${p.avg_ms.toFixed(0)} ms` : "—"}</td>
                      <td className="r num muted">{p.min_ms != null ? `${p.min_ms} / ${p.max_ms} ms` : "—"}</td>
                      <td className="r num">{p.jitter_ms != null ? `${p.jitter_ms.toFixed(1)} ms` : "—"}</td>
                      <td className="r num" style={{ color: p.loss_percent >= 5 ? "var(--ink-critical)" : p.loss_percent > 0 ? "var(--ink-warning)" : undefined }}>
                        {p.loss_percent.toFixed(0)}%
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="stack" style={{ gap: 8 }}>
                <div className="small muted">DNS lookup time</div>
                {test.dns.map((d) => (
                  <div key={d.host} className="row between small">
                    <span className="mono">{d.host}</span>
                    <span className="num">{d.ms != null ? `${d.ms} ms` : <span style={{ color: "var(--ink-critical)" }}>{d.error ?? "failed"}</span>}</span>
                  </div>
                ))}
                <Verdict report={test} />
              </div>
            </div>
          )}
        </Panel>
      )}

      <div className="mt-2"><TestHistory version={historyVersion} /></div>

      <div className="grid mt-2" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)", alignItems: "start" }}>
        <DnsPanel adapters={net.data?.adapters ?? []} onChanged={net.reload} />
        <TraceroutePanel />
      </div>

      <section className="panel flush mt-2">
        <div className="panel-head">
          <div>
            <h2>Adapters</h2>
            <div className="sub">Status, link speed, gateway and DNS servers for each adapter</div>
          </div>
          {hidden > 0 && (
            <button className="btn ghost sm" onClick={() => setShowVirtual((v) => !v)}>
              {showVirtual ? "Hide" : "Show"} {hidden} virtual
            </button>
          )}
        </div>
        {net.error && <div style={{ padding: 16 }}><ErrorNote message={net.error} onRetry={net.reload} /></div>}
        {net.loading && !net.data && <Loading />}
        {net.data && adapters.length === 0 && <Empty title="No adapters found" />}
        {adapters.length > 0 && (
          <table className="table">
            <thead>
              <tr>
                <th>Adapter</th>
                <th>Status</th>
                <th>Link</th>
                <th>Gateway</th>
                <th>DNS servers</th>
              </tr>
            </thead>
            <tbody>
              {adapters.map((a) => (
                <AdapterRow key={a.index} a={a} />
              ))}
            </tbody>
          </table>
        )}
        {net.data?.warnings.map((w, i) => (
          <div key={i} className="small dim" style={{ padding: "8px 18px" }}>
            {w}
          </div>
        ))}
      </section>

      <div className="section-title">Network optimisations</div>
      <section className="panel flush">
        {tweaks.error && <div style={{ padding: 16 }}><ErrorNote message={tweaks.error} onRetry={tweaks.reload} /></div>}
        {tweaks.data && <TweakList tweaks={tweaks.data} category="network" />}
      </section>

      <div className="section-title">If problems persist</div>
      <Panel>
        <div className="small muted stack" style={{ gap: 8 }}>
          <div>
            A full network reset reinstalls adapters and removes saved Wi-Fi networks and VPN adapters. NOCTURN doesn't run it for you because it disconnects you and needs a restart — but if loss to your <b>router</b> is high, it's worth trying after updating the adapter driver:
          </div>
          <div>Settings › Network &amp; internet › Advanced network settings › <b>Network reset</b></div>
          <div>If loss appears only on internet targets and not the router, the problem is usually outside this PC (router, ISP or Wi-Fi interference).</div>
        </div>
      </Panel>
    </div>
  );
}

function AdapterRow({ a }: { a: Adapter }) {
  const up = a.status.toLowerCase() === "up";
  return (
    <tr>
      <td style={{ maxWidth: 0, width: "34%" }}>
        <div className="row" style={{ gap: 8 }}>
          <span className="status-dot" style={{ background: up ? "var(--good)" : "var(--ink-3)" }} />
          <span style={{ fontWeight: 500 }}>{a.name}</span>
          {a.is_virtual && <span className="tag">Virtual</span>}
        </div>
        <div className="small dim truncate" title={a.description}>
          {a.description}
          {a.driver_version ? ` · driver ${a.driver_version}` : ""}
        </div>
      </td>
      <td className="small">{a.status}</td>
      <td className="small num">{a.link_speed ?? "—"}</td>
      <td className="mono small">{a.gateway ?? "—"}</td>
      <td className="mono small selectable">{a.dns_servers.length ? a.dns_servers.join(", ") : "—"}</td>
    </tr>
  );
}

function Verdict({ report }: { report: StabilityReport }) {
  const router = report.pings.find((p) => /router|gateway/i.test(p.label));
  const internet = report.pings.filter((p) => p !== router);
  const worstLoss = Math.max(0, ...internet.map((p) => p.loss_percent));
  const worstJitter = Math.max(0, ...report.pings.map((p) => p.jitter_ms ?? 0));
  if (router && router.loss_percent >= 2)
    return <Note tone="warning">Packets are being lost between this PC and your router ({router.loss_percent.toFixed(0)}%). That points to Wi-Fi signal, a cable, or the adapter driver — not your ISP.</Note>;
  if (worstLoss >= 2)
    return <Note tone="warning">Your local link is clean, but {worstLoss.toFixed(0)}% of packets to the internet were lost. The cause is likely the router or your ISP.</Note>;
  if (worstJitter >= 20)
    return <Note tone="warning">Latency varies a lot ({worstJitter.toFixed(0)} ms jitter), which shows up as lag spikes in games and calls. Wi-Fi interference or heavy uploads are common causes.</Note>;
  return <Note tone="good">No packet loss and stable latency in this test.</Note>;
}
