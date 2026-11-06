// Home dashboard widgets. Each widget is self-contained and reads live data
// itself; the dashboard only decides which ones appear, in what order and at
// what size (all stored in preferences).

import { useState, type ReactNode } from "react";
import { I } from "../components/Icons";
import { LineChart } from "../components/LineChart";
import { Meter } from "../components/ui";
import { api, library } from "../lib/api";
import { AppIcon, launchItem, whereItOpens } from "../library/tiles";
import { useApp, useLoad } from "../lib/app";
import { bytes, pct, rate } from "../lib/format";
import { primaryGpu, useTelemetry } from "../lib/telemetry";
import { usePrefs } from "./PrefsProvider";
import { useMusic } from "../music/MusicProvider";
import { Art } from "../music/Player";
import type { WidgetType } from "./prefs";

export interface WidgetDef {
  type: WidgetType;
  name: string;
  description: string;
  render: () => ReactNode;
}

function Card({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <div className="widget">
      <div className="widget-head">
        <span>{title}</span>
        {action}
      </div>
      <div className="widget-body">{children}</div>
    </div>
  );
}

function Go({ to, label }: { to: Parameters<ReturnType<typeof useApp>["navigate"]>[0]; label: string }) {
  const { navigate } = useApp();
  return (
    <button className="btn ghost sm" onClick={() => navigate(to)}>
      {label} <I.arrow />
    </button>
  );
}

function Vitals() {
  const { latest, history } = useTelemetry();
  const gpu = primaryGpu(latest);
  const mem = latest ? (latest.mem_used / Math.max(1, latest.mem_total)) * 100 : 0;
  return (
    <Card title="System" action={<Go to="hardware" label="Hardware" />}>
      <div className="grid cols-3" style={{ gap: 16 }}>
        <div className="stack" style={{ gap: 6 }}>
          <div className="row between small"><span className="muted">CPU</span><b className="num">{latest ? pct(latest.cpu_usage) : "—"}</b></div>
          <Meter value={latest?.cpu_usage ?? 0} label="CPU usage" />
        </div>
        <div className="stack" style={{ gap: 6 }}>
          <div className="row between small"><span className="muted">Memory</span><b className="num">{latest ? pct(mem) : "—"}</b></div>
          <Meter value={mem} label="Memory in use" />
        </div>
        <div className="stack" style={{ gap: 6 }}>
          <div className="row between small">
            <span className="muted">GPU{gpu?.temperature_c != null ? ` · ${gpu.temperature_c}°C` : ""}</span>
            <b className="num">{gpu?.usage != null ? pct(gpu.usage) : "—"}</b>
          </div>
          <Meter value={gpu?.usage ?? 0} label="GPU usage" />
        </div>
      </div>
      {history.length > 1 && (
        <div className="mt-2">
          <LineChart
            ariaLabel="CPU usage, last two minutes"
            series={[{ key: "cpu", label: "CPU", color: "var(--series-1)", values: history.map((s) => s.cpu_usage) }]}
            times={history.map((s) => s.ts)}
            max={100}
            format={(v) => `${Math.round(v)}%`}
            height={64}
          />
        </div>
      )}
    </Card>
  );
}

function Quick() {
  const { requestApply, navigate, dataVersion } = useApp();
  const plans = useLoad(() => api.powerPlans(), [dataVersion]);
  const profiles = useLoad(() => api.appProfiles(), [dataVersion]);
  const active = plans.data?.find((p) => p.active);
  return (
    <Card title="Quick actions">
      <div className="small muted">
        Power plan: <span style={{ color: "var(--ink-0)" }}>{active?.name ?? "—"}</span>
        {profiles.data?.active && <> · profile <span style={{ color: "var(--ink-0)" }}>{profiles.data.active.label}</span> active</>}
      </div>
      <div className="chips mt-2">
        <button className="chip" onClick={() => navigate("gaming")}>Gaming mode</button>
        <button
          className="chip"
          onClick={() => requestApply({ tweakIds: ["performance.clean_temp"], source: "user", label: "Clean temporary files", title: "Clean temporary files" })}
        >
          Clean temp files
        </button>
        <button className="chip" onClick={() => navigate("benchmarks")}>Run a benchmark</button>
        <button className="chip" onClick={() => navigate("startup")}>Startup apps</button>
      </div>
    </Card>
  );
}

function Health() {
  const { dataVersion } = useApp();
  const h = useLoad(() => api.health(false), [dataVersion]);
  const findings = h.data?.findings ?? [];
  const serious = findings.filter((f) => f.severity !== "info");
  return (
    <Card title="Health" action={<Go to="overview" label="Details" />}>
      {!h.data ? (
        <div className="small dim">Checking…</div>
      ) : serious.length === 0 ? (
        <div className="row" style={{ gap: 8 }}><span className="tag good"><I.check /> Healthy</span><span className="small muted">Nothing needs attention.</span></div>
      ) : (
        <div className="stack" style={{ gap: 6 }}>
          {serious.slice(0, 3).map((f) => (
            <div key={f.id} className="small">
              <span className={`tag ${f.severity === "critical" ? "critical" : "warning"}`} style={{ marginRight: 8 }}>{f.severity}</span>
              {f.title}
            </div>
          ))}
          {serious.length > 3 && <div className="small dim">and {serious.length - 3} more</div>}
        </div>
      )}
    </Card>
  );
}

function Storage() {
  const { latest } = useTelemetry();
  const disks = (latest?.disks ?? []).filter((d) => !d.removable && d.total > 0);
  return (
    <Card title="Storage" action={<Go to="storage" label="Open" />}>
      <div className="stack" style={{ gap: 8 }}>
        {disks.map((d) => {
          const used = ((d.total - d.available) / d.total) * 100;
          return (
            <div key={d.mount} className="stack" style={{ gap: 4 }}>
              <div className="row between small"><span>{d.mount}</span><span className="muted num">{bytes(d.available)} free</span></div>
              <Meter value={used} label={`${d.mount} used`} warnAt={85} critAt={93} />
            </div>
          );
        })}
        {disks.length === 0 && <div className="small dim">Reading drives…</div>}
      </div>
    </Card>
  );
}

function Network() {
  const { latest } = useTelemetry();
  return (
    <Card title="Network" action={<Go to="network" label="Open" />}>
      <div className="stack" style={{ gap: 6 }}>
        <div className="row between small"><span className="muted">Down</span><b className="num">{latest ? `${rate(latest.net_rx_bps)}` : "—"}</b></div>
        <div className="row between small"><span className="muted">Up</span><b className="num">{latest ? `${rate(latest.net_tx_bps)}` : "—"}</b></div>
      </div>
    </Card>
  );
}

function Music() {
  const m = useMusic();
  const t = m.current;
  return (
    <Card title="Nocturn Music" action={<Go to="music" label="Open" />}>
      {t ? (
        <div className="row" style={{ gap: 12 }}>
          <Art track={t} size={52} />
          <div className="stack" style={{ gap: 2, minWidth: 0, flex: 1 }}>
            <b className="truncate">{t.title}</b>
            <span className="small muted truncate">{t.artist}</span>
          </div>
          <button className="btn ghost icon sm" aria-label="Previous" onClick={m.prev}><I.prev /></button>
          <button className="btn primary icon" aria-label={m.playing ? "Pause" : "Play"} onClick={m.toggle}>{m.playing ? <I.pause /> : <I.play />}</button>
          <button className="btn ghost icon sm" aria-label="Next" onClick={m.next}><I.next /></button>
        </div>
      ) : (
        <div className="row" style={{ gap: 12 }}>
          <div className="art-placeholder"><I.music /></div>
          <div className="small muted">{m.tracks.length ? `Nothing playing. ${m.tracks.length} songs in your library.` : "Add a music folder in Music › Sources to start your library."}</div>
        </div>
      )}
    </Card>
  );
}

function Apps() {
  const { toast } = useApp();
  const items = useLoad(() => library.list(), []);
  const { prefs } = usePrefs();
  const all = items.data ?? [];
  const ws = prefs.workspaces.list.find((w) => w.id === prefs.workspaces.active);
  // In a workspace: its own apps. Otherwise pinned first, then most recently opened.
  const recent = all.filter((i) => !i.pinned && i.last_opened_at).sort((a, b) => (b.last_opened_at ?? "").localeCompare(a.last_opened_at ?? ""));
  const shown = (ws ? ws.apps.map((id) => all.find((i) => i.id === id)).filter((i): i is NonNullable<typeof i> => !!i) : [...all.filter((i) => i.pinned), ...recent]).slice(0, 8);
  return (
    <Card title={ws ? `${ws.name} apps` : "Your apps"} action={<Go to={ws ? "workspaces" : "library"} label={ws ? "Workspace" : "Library"} />}>
      {shown.length === 0 ? (
        <div className="small muted">Pin apps in your Library, or open some, and they'll appear here.</div>
      ) : (
        <div className="mini-apps">
          {shown.map((i) => (
            <button key={i.id} className="mini-app" title={`${i.name} — ${whereItOpens(i)}`} onClick={() => launchItem(i, toast, items.reload)}>
              <AppIcon item={i} size={36} />
              <span>{i.name}</span>
            </button>
          ))}
        </div>
      )}
    </Card>
  );
}

function Ask() {
  const { navigate } = useApp();
  const [q, setQ] = useState("");
  const go = () => q.trim() && navigate("assistant", { prompt: q.trim() });
  return (
    <Card title="Ask Nocturn AI">
      <div className="row" style={{ gap: 8 }}>
        <input className="input" style={{ flex: 1 }} placeholder="e.g. Why is my game stuttering?" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.key === "Enter" && go()} aria-label="Ask Nocturn AI" />
        <button className="btn primary" disabled={!q.trim()} onClick={go}><I.assistant /></button>
      </div>
    </Card>
  );
}

function Gaming() {
  const now = useLoad(() => api.gamingNow(), []);
  const live = now.data?.live;
  return (
    <Card title="Gaming" action={<Go to="gaming" label="Open" />}>
      {live ? (
        <div className="small"><span className="tag good">Playing</span> <b>{live.game_name}</b></div>
      ) : (
        <div className="small muted">No game running.</div>
      )}
    </Card>
  );
}

export const WIDGETS: WidgetDef[] = [
  { type: "vitals", name: "System vitals", description: "CPU, memory and GPU with a two-minute chart", render: () => <Vitals /> },
  { type: "quick", name: "Quick actions", description: "Power plan, active profile and shortcuts", render: () => <Quick /> },
  { type: "health", name: "Health", description: "Issues NOCTURN's health check found", render: () => <Health /> },
  { type: "music", name: "Now playing", description: "Nocturn Music", render: () => <Music /> },
  { type: "apps", name: "Your apps", description: "Pinned and recently opened apps from your Library", render: () => <Apps /> },
  { type: "storage", name: "Storage", description: "Free space per drive", render: () => <Storage /> },
  { type: "network", name: "Network", description: "Live download and upload", render: () => <Network /> },
  { type: "ask", name: "Ask AI", description: "A quick question for Nocturn AI", render: () => <Ask /> },
  { type: "gaming", name: "Gaming", description: "The game you're playing", render: () => <Gaming /> },
];

export const widgetDef = (t: WidgetType) => WIDGETS.find((w) => w.type === t)!;
