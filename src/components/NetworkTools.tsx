// Network page tools: DNS provider switching (journaled, admin), traceroute
// (streamed hop by hop) and stability-test history.

import { useEffect, useMemo, useState } from "react";
import { api, errorMessage, type Adapter, type DnsPreviewItem, type Hop, type TraceReport } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { date } from "../lib/format";
import { I } from "./Icons";
import { LineChart } from "./LineChart";
import { Empty, ErrorNote, Loading, Note, Panel, Spinner } from "./ui";

// ------------------------------------------------------------------ DNS

type Phase = "edit" | "previewing" | "review" | "running";

export function DnsPanel({ adapters, onChanged }: { adapters: Adapter[]; onChanged: () => void }) {
  const { toast, bumpData } = useApp();
  const providers = useLoad(() => api.dnsProviders(), []);
  const physical = adapters.filter((a) => !a.is_virtual);
  const candidates = physical.some((a) => a.status.toLowerCase() === "up") ? physical.filter((a) => a.status.toLowerCase() === "up") : physical;
  const [index, setIndex] = useState<number | null>(null);
  const [provider, setProvider] = useState("cloudflare");
  const [phase, setPhase] = useState<Phase>("edit");
  const [items, setItems] = useState<DnsPreviewItem[]>([]);
  const [error, setError] = useState<string | null>(null);

  // Default to the adapter carrying the default route.
  useEffect(() => {
    if (index == null && candidates.length) setIndex((candidates.find((a) => a.gateway) ?? candidates[0]).index);
  }, [candidates, index]);

  const adapter = candidates.find((a) => a.index === index) ?? null;
  const changes = items.filter((i) => i.will_change).length;
  const pick = <T,>(set: (v: T) => void) => (v: T) => {
    set(v);
    setPhase("edit");
    setError(null);
  };

  async function preview() {
    if (!adapter) return;
    setPhase("previewing");
    setError(null);
    try {
      setItems(await api.previewDns(adapter.index, adapter.name, provider));
      setPhase("review");
    } catch (e) {
      setError(errorMessage(e));
      setPhase("edit");
    }
  }

  async function apply() {
    if (!adapter) return;
    setPhase("running");
    try {
      const r = await api.setDns(adapter.index, adapter.name, provider);
      const ok = r.status === "completed";
      const undoable = ok || r.status === "partial";
      const err = r.results.find((x) => x.error)?.error;
      const name = providers.data?.find((p) => p.id === provider)?.name ?? provider;
      toast({
        tone: ok ? "good" : "warning",
        title: ok
          ? `${adapter.name} now uses ${name}`
          : r.status === "partial"
            ? "DNS partly changed"
            : r.status === "nothing_to_do"
              ? "DNS already set"
              : "DNS not changed",
        detail: ok ? "Takes effect immediately; the DNS cache was cleared." : err ?? r.notes[0],
        action: undoable ? { label: "Undo", run: () => api.rollback(r.tx_id).then(() => { bumpData(); onChanged(); }) } : undefined,
      });
      bumpData();
      onChanged();
      setPhase("edit");
      setItems([]);
    } catch (e) {
      setError(errorMessage(e));
      setPhase("review");
    }
  }

  const busy = phase === "previewing" || phase === "running";

  return (
    <Panel title="DNS servers" sub="Which resolver turns names into addresses. Changes need administrator approval and can be undone from History.">
      {providers.error && <ErrorNote message={providers.error} onRetry={providers.reload} />}
      {candidates.length === 0 ? (
        <div className="small muted">No physical network adapter found.</div>
      ) : (
        <div className="stack" style={{ gap: 14 }}>
          <div className="field">
            <label htmlFor="dnsad">Adapter</label>
            <select id="dnsad" className="select" value={index ?? ""} disabled={busy} onChange={(e) => pick(setIndex)(Number(e.target.value))}>
              {candidates.map((a) => (
                <option key={a.index} value={a.index}>{a.name}{a.gateway ? " (internet)" : ""}</option>
              ))}
            </select>
            <div className="hint mono">In use now: {adapter?.dns_servers.length ? adapter.dns_servers.join(", ") : "—"}</div>
          </div>

          <div className="list" role="radiogroup" aria-label="DNS provider">
            {providers.data?.map((p) => (
              <label key={p.id} className="list-item" style={{ alignItems: "center", cursor: busy ? "default" : "pointer", background: provider === p.id ? "var(--bg-3)" : undefined }}>
                <input type="radio" name="dnsprov" value={p.id} checked={provider === p.id} disabled={busy} onChange={() => pick(setProvider)(p.id)} />
                <div className="body">
                  <div className="title">{p.name}</div>
                  <div className="desc">{p.note}</div>
                </div>
                {p.v4.length > 0 && <span className="small mono dim">{p.v4[0]}</span>}
              </label>
            ))}
          </div>

          {error && <ErrorNote message={error} />}

          {phase === "review" && (
            <div className="stack" style={{ gap: 0 }}>
              {items.map((i) => (
                <div key={i.target} className="stack" style={{ gap: 3, padding: "10px 0", borderTop: "1px solid var(--line)", opacity: i.will_change ? 1 : 0.55 }}>
                  <div className="row between">
                    <span className="title small">{i.target}</span>
                    {!i.will_change && <span className="tag solid">Already set</span>}
                  </div>
                  <div className="small"><span className="dim">Now:</span> {i.current} <span className="dim">→</span> {i.proposed}</div>
                </div>
              ))}
              <p className="small dim" style={{ margin: "8px 0 0" }}>
                "Automatic" means the servers your router hands out. Windows will ask for administrator approval.
              </p>
            </div>
          )}

          <div className="row" style={{ justifyContent: "flex-end", gap: 6 }}>
            {phase === "review" || phase === "running" ? (
              <>
                <button className="btn ghost" disabled={busy} onClick={() => setPhase("edit")}>Back</button>
                <button className="btn primary" disabled={busy || changes === 0} onClick={apply}>
                  {phase === "running" ? <Spinner /> : <I.shield />} {changes === 0 ? "Nothing to change" : "Apply"}
                </button>
              </>
            ) : (
              <button className="btn primary" disabled={busy || !adapter} onClick={preview}>
                {busy ? <Spinner /> : null} Preview <I.arrow />
              </button>
            )}
          </div>
          <div className="small dim">A different DNS provider can speed up page loads slightly or add filtering. It doesn't change ping in games — that's set by the route to the game server.</div>
        </div>
      )}
    </Panel>
  );
}

// ------------------------------------------------------------ traceroute

export function TraceroutePanel() {
  const [host, setHost] = useState("1.1.1.1");
  const [hops, setHops] = useState<Hop[]>([]);
  const [report, setReport] = useState<TraceReport | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run() {
    setRunning(true);
    setError(null);
    setReport(null);
    setHops([]);
    try {
      const r = await api.traceroute(host, (h) => setHops((prev) => [...prev, h]));
      setReport(r);
      setHops(r.hops);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRunning(false);
    }
  }

  const ms = (v: number | null) => (v == null ? "*" : v < 1 ? "<1" : String(v));

  return (
    <Panel title="Route to a server" sub="Traceroute: every router between this PC and the destination, with response times">
      <div className="row" style={{ gap: 6 }}>
        <input
          className="input mono"
          style={{ flex: 1 }}
          aria-label="Host name or IPv4 address"
          value={host}
          disabled={running}
          onChange={(e) => setHost(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !running && host.trim() && run()}
          placeholder="example.com or 1.1.1.1"
        />
        <button className="btn primary" disabled={running || !host.trim()} onClick={run}>
          {running ? <Spinner /> : null} {running ? "Tracing…" : "Trace"}
        </button>
      </div>
      {error && <div className="mt-2"><ErrorNote message={error} /></div>}
      {(hops.length > 0 || running) && (
        <div className="table-wrap mt-2" style={{ maxHeight: 360 }}>
          <table className="table">
            <thead>
              <tr>
                <th className="r">Hop</th>
                <th>Router</th>
                <th className="r">Probe 1</th>
                <th className="r">Probe 2</th>
                <th className="r">Probe 3</th>
              </tr>
            </thead>
            <tbody>
              {hops.map((h) => (
                <tr key={h.ttl}>
                  <td className="r num">{h.ttl}</td>
                  <td className="mono small">
                    {h.address ?? <span className="dim">no reply</span>}
                    {h.private && <span className="tag" style={{ marginLeft: 6 }}>local network</span>}
                    {h.reached && <span className="tag good" style={{ marginLeft: 6 }}>destination</span>}
                  </td>
                  {[0, 1, 2].map((i) => (
                    <td key={i} className="r num small">{h.address ? `${ms(h.rtts[i] ?? null)} ms` : "*"}</td>
                  ))}
                </tr>
              ))}
              {running && (
                <tr>
                  <td colSpan={5} className="small dim"><Spinner /> Probing hop {hops.length + 1}…</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
      {report && (
        <div className="small muted mt-2">
          {report.reached ? `Reached ${report.address} in ${report.hops.length} hop${report.hops.length === 1 ? "" : "s"}.` : report.note}
          {" "}A single slow or silent hop is normal — routers often deprioritise these probes. Only a jump that stays high for every later hop points to a real problem.
        </div>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------- history

export function TestHistory({ version }: { version: number }) {
  const hist = useLoad(() => api.networkHistory(600), [version]);

  const { times, router, internet, lossTests, total } = useMemo(() => {
    const byTest = new Map<string, { router: number | null; internet: number[]; loss: boolean }>();
    for (const p of [...(hist.data ?? [])].reverse()) {
      const t = byTest.get(p.tested_at) ?? { router: null, internet: [], loss: false };
      if (/router|gateway/i.test(p.label)) t.router = p.avg_ms;
      else if (p.avg_ms != null) t.internet.push(p.avg_ms);
      t.loss ||= p.loss_percent > 0;
      byTest.set(p.tested_at, t);
    }
    const entries = [...byTest.entries()];
    return {
      times: entries.map(([at]) => Date.parse(at)),
      router: entries.map(([, t]) => t.router),
      internet: entries.map(([, t]) => (t.internet.length ? t.internet.reduce((a, b) => a + b, 0) / t.internet.length : null)),
      lossTests: entries.filter(([, t]) => t.loss).length,
      total: entries.length,
    };
  }, [hist.data]);

  return (
    <Panel title="Latency history" sub={total ? `${total} stability test${total === 1 ? "" : "s"} recorded` : "Every stability test is saved here"}>
      {hist.error && <ErrorNote message={hist.error} onRetry={hist.reload} />}
      {hist.loading && !hist.data && <Loading />}
      {hist.data && total < 2 && (
        <Empty title={total === 0 ? "No tests yet" : "One test so far"}>Run the stability test at different times of day to see whether lag comes and goes.</Empty>
      )}
      {total >= 2 && (
        <>
          <LineChart
            ariaLabel="Average latency per stability test"
            series={[
              { key: "router", label: "Router", color: "var(--series-1)", values: router },
              { key: "internet", label: "Internet", color: "var(--series-2)", values: internet },
            ]}
            times={times}
            format={(v) => `${Math.round(v)} ms`}
            timeLabel={(ts) => date(new Date(ts).toISOString(), true)}
            height={150}
            ticks={3}
            area={false}
          />
          <div className="small dim mt-2">
            Average round-trip time per test; "Internet" is the mean of the public resolvers.{" "}
            {lossTests > 0 ? `${lossTests} of ${total} tests saw packet loss.` : "No test so far has seen packet loss."}
          </div>
        </>
      )}
      {hist.data && total > 0 && lossTests / total >= 0.3 && (
        <div className="mt-2"><Note tone="warning">Packet loss shows up in many of your tests. Check whether it's on the router line (local Wi-Fi or cable) or only on internet targets (router or ISP).</Note></div>
      )}
    </Panel>
  );
}
