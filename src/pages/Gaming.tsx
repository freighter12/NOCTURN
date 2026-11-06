// Gaming optimiser: live session and bottleneck monitor, overlay detection,
// gaming modes (previewed through the shared ApplyFlow), detected game library
// with per-game profiles, session history, readiness and power plans.

import { useEffect, useRef, useState } from "react";
import { GameProfileEditor } from "../components/GameProfileEditor";
import { I } from "../components/Icons";
import { ProcessTable } from "../components/ProcessTable";
import { TweakList } from "../components/TweakList";
import { Empty, ErrorNote, Loading, Meter, Note, PageHead, Panel, Spinner } from "../components/ui";
import { api, errorMessage, type GamingNow, type GameView, type SessionSummary, type TweakView } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { bytes, date, duration } from "../lib/format";
import { primaryGpu, useTelemetry } from "../lib/telemetry";

const READINESS: { id: string; label: string; good: string; bad: string }[] = [
  { id: "gaming.game_mode", label: "Game Mode", good: "On", bad: "Off" },
  { id: "gaming.disable_background_recording", label: "Background recording", good: "Off", bad: "May be on" },
  { id: "gaming.power_high_performance", label: "High performance power plan", good: "Active", bad: "Not active" },
  { id: "gaming.hardware_gpu_scheduling", label: "Hardware GPU scheduling", good: "On", bad: "Off" },
  { id: "gaming.mouse_precision_off", label: "Mouse acceleration", good: "Off", bad: "On" },
];

export function Gaming() {
  const { requestApply, dataVersion, navigate } = useApp();
  const tweaks = useLoad(() => api.tweaks(), [dataVersion]);
  const profiles = useLoad(() => api.profiles(), []);

  const byId = new Map((tweaks.data ?? []).map((t) => [t.id, t] as [string, TweakView]));
  const gaming = profiles.data?.find((p) => p.id === "gaming");
  const modes = profiles.data?.filter((p) => p.kind === "gaming_mode") ?? [];

  // Session history reloads whenever a live session ends.
  const [sessionsVersion, setSessionsVersion] = useState(0);

  return (
    <div className="page">
      <PageHead
        eyebrow="Gaming"
        title="Gaming optimisation"
        desc="See what limits your games while you play, set per-game priority and graphics options, and apply a gaming mode with a full preview first."
        actions={
          <button className="btn" onClick={() => navigate("assistant", { prompt: "Why am I getting low FPS or stutter in games? Please check my system." })}>
            <I.assistant /> Ask AI about FPS
          </button>
        }
      />

      <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1.25fr) minmax(0, 1fr)" }}>
        <LiveSession onSessionEnded={() => setSessionsVersion((v) => v + 1)} />

        <Panel title="Gaming readiness" sub="Read live from Windows">
          {tweaks.loading && !tweaks.data && <Loading />}
          {tweaks.error && <ErrorNote message={tweaks.error} onRetry={tweaks.reload} />}
          <div className="list">
            {READINESS.map((r) => {
              const t = byId.get(r.id);
              if (!t) return null;
              const ok = t.status.state === "applied";
              const unavailable = t.status.state === "unavailable";
              return (
                <div className="list-item" key={r.id} style={{ alignItems: "center" }}>
                  <span className="status-dot" style={{ background: unavailable ? "var(--ink-3)" : ok ? "var(--good)" : "var(--warning)" }} />
                  <div className="body">
                    <div className="title">{r.label}</div>
                    {t.status.detail && <div className="desc">{t.status.detail}</div>}
                  </div>
                  <span className="small" style={{ color: ok ? "var(--ink-0)" : "var(--ink-2)" }}>
                    {unavailable ? "Unavailable" : ok ? r.good : r.bad}
                  </span>
                </div>
              );
            })}
          </div>
          {gaming && (
            <button
              className="btn mt-2"
              onClick={() =>
                requestApply({ tweakIds: gaming.tweaks, source: "profile:gaming", label: "Gaming profile", title: "Gaming profile", description: "Preview every change. Settings already configured are skipped; everything reversible can be rolled back." })
              }
            >
              Preview Gaming profile <I.arrow />
            </button>
          )}
        </Panel>
      </div>

      <div className="section-title">Gaming modes</div>
      <div className="grid cols-4">
        {profiles.loading && !profiles.data && <Loading />}
        {modes.map((m) => {
          const views = tweaks.data?.filter((t) => m.tweaks.includes(t.id)) ?? [];
          const done = views.filter((t) => t.status.state === "applied").length;
          const active = tweaks.data && done === m.tweaks.length;
          return (
            <Panel key={m.id} lit={!!active}>
              <div className="stack" style={{ gap: 8, height: "100%" }}>
                <div className="row between">
                  <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>{m.name}</h3>
                  {active ? <span className="tag good"><I.check /> In place</span> : <span className="dim small num">{m.tweaks.length} settings</span>}
                </div>
                <div className="small muted" style={{ flex: 1 }}>{m.description}</div>
                <div className="small dim">{tweaks.data ? `${done} of ${m.tweaks.length} already set` : "…"}</div>
                <button
                  className="btn"
                  disabled={!!active}
                  onClick={() =>
                    requestApply({
                      tweakIds: m.tweaks,
                      source: `mode:${m.id.replace(/^mode\./, "")}`,
                      label: `${m.name} gaming mode`,
                      title: `${m.name} gaming mode`,
                      description: m.description,
                    })
                  }
                >
                  Preview <I.arrow />
                </button>
              </div>
            </Panel>
          );
        })}
      </div>
      <p className="small dim">
        Modes only switch Windows settings on — applying one doesn't undo another. To go back, roll changes back from History. Compare frame rates in the same scene before and after.
      </p>

      <GameLibrary />

      <SessionHistory version={sessionsVersion} />

      <PowerPlans />

      <section className="panel flush mt-2">
        <div className="panel-head">
          <div>
            <h2>Resource-heavy background processes</h2>
            <div className="sub">Close what you don't need before launching a game. Unsaved work in an app you end will be lost.</div>
          </div>
        </div>
        <ProcessTable limit={12} />
      </section>

      <div className="section-title">Gaming optimisations</div>
      <section className="panel flush">{tweaks.data && <TweakList tweaks={tweaks.data} category="gaming" />}</section>
    </div>
  );
}

// ---------------------------------------------------------- live session

function LiveSession({ onSessionEnded }: { onSessionEnded: () => void }) {
  const { latest } = useTelemetry();
  const [now, setNow] = useState<GamingNow | null>(null);
  const [error, setError] = useState<string | null>(null);
  const liveId = useRef<string | null>(null);

  useEffect(() => {
    let alive = true;
    const poll = () =>
      api
        .gamingNow()
        .then((n) => {
          if (!alive) return;
          const id = n.live ? `${n.live.game_id}@${n.live.started_at}` : null;
          if (liveId.current && liveId.current !== id) onSessionEnded();
          liveId.current = id;
          setNow(n);
          setError(null);
        })
        .catch((e) => alive && setError(errorMessage(e)));
    poll();
    const t = setInterval(poll, 2000);
    return () => {
      alive = false;
      clearInterval(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const live = now?.live;
  const gpu = primaryGpu(latest);
  const busiestCore = latest?.cpu_per_core.length ? Math.max(...latest.cpu_per_core) : null;

  return (
    <Panel lit={!!live} title={live ? live.game_name : "Live session"} sub={live ? `Playing for ${duration(Math.max(0, (live.ended_at - live.started_at) / 1000))}` : "Starts automatically when a detected game runs"}>
      {error && <ErrorNote message={error} />}
      {!now && !error && <Loading />}
      {now && !live && (
        <Empty title="No game running">
          Launch a game from your library below — NOCTURN tracks CPU and GPU load while you play and works out what's limiting your frame rate. Nothing is changed during a session.
        </Empty>
      )}
      {live && (
        <div className="stack" style={{ gap: 14 }}>
          <div className="grid cols-2" style={{ gap: 12 }}>
            <LoadBar label="GPU now" value={gpu?.usage ?? null} />
            <LoadBar label="Busiest CPU core now" value={busiestCore} />
          </div>
          <dl className="kv">
            <dt>Game process</dt>
            <dd className="num">{now?.game_cpu != null ? `${now.game_cpu.toFixed(1)}% CPU` : "—"} · {bytes(now?.game_mem ?? 0)}</dd>
            <dt>Session average</dt>
            <dd className="num">
              CPU {live.avg_cpu.toFixed(0)}% · busiest core {live.avg_max_core.toFixed(0)}% · GPU {live.avg_gpu != null ? `${live.avg_gpu.toFixed(0)}%` : "n/a"}
            </dd>
            {live.peak_gpu_temp != null && (
              <>
                <dt>GPU temperature peak</dt>
                <dd className="num">{live.peak_gpu_temp.toFixed(0)} °C</dd>
              </>
            )}
            <dt>Time limited by</dt>
            <dd className="num">GPU {live.gpu_bound_pct.toFixed(0)}% · CPU {live.cpu_bound_pct.toFixed(0)}%</dd>
          </dl>
          <Note>{live.verdict}</Note>
          <div className="small dim">Estimated from usage, not frame times: a GPU above 95% means the GPU sets the pace; a GPU below 80% while one CPU core is above 90% points to the CPU.</div>
        </div>
      )}
      {now && now.overlays.length > 0 && (
        <div className="mt-2">
          <div className="small muted" style={{ marginBottom: 6 }}>Overlays and capture tools running</div>
          <div className="chips">
            {now.overlays.map((o) => (
              <span key={o.label} className="tag" title={o.process}>{o.label}</span>
            ))}
          </div>
          <div className="small dim mt-2">Each draws over or captures every frame. Most cost little, but if a game stutters, turn off the overlays you don't use in their own settings.</div>
        </div>
      )}
    </Panel>
  );
}

function LoadBar({ label, value }: { label: string; value: number | null }) {
  return (
    <div className="stack" style={{ gap: 6 }}>
      <div className="row between small">
        <span className="muted">{label}</span>
        <span className="num">{value != null ? `${value.toFixed(0)}%` : "n/a"}</span>
      </div>
      {/* No warning colours: high load in a game is expected, not a problem. */}
      <Meter label={label} value={value ?? 0} warnAt={101} critAt={101} />
    </div>
  );
}

// ---------------------------------------------------------- library

function GameLibrary() {
  const { toast, dataVersion } = useApp();
  const [refreshTick, setRefreshTick] = useState(0);
  const games = useLoad(() => api.games(refreshTick > 0), [refreshTick, dataVersion]);
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [showHidden, setShowHidden] = useState(false);

  // Keep the running badge fresh without re-scanning launchers.
  useEffect(() => {
    const t = setInterval(() => api.games(false).then(games.setData).catch(() => {}), 10_000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const list = (games.data ?? []).filter((g) => !g.not_game);
  const hidden = (games.data ?? []).filter((g) => g.not_game);
  const current = list.find((g) => g.id === selected) ?? null;

  async function markNotGame(g: GameView, notGame: boolean) {
    try {
      await api.setNotGame(g.id, notGame);
      if (notGame && selected === g.id) setSelected(null);
      toast({
        tone: "good",
        title: notGame ? `${g.name} won't be treated as a game` : `${g.name} restored`,
        detail: notGame ? "It won't start game sessions or block benchmarks. Restore it from the bottom of the list." : undefined,
        action: notGame ? { label: "Undo", run: () => api.setNotGame(g.id, false).then(() => games.reload()) } : undefined,
      });
      games.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't update", detail: errorMessage(e) });
    }
  }

  async function add() {
    const exe = path.trim().replace(/^"(.*)"$/, "$1");
    if (!exe) return;
    setBusy(true);
    try {
      await api.addCustomGame(exe);
      toast({ tone: "good", title: "Game added", detail: exe });
      setPath("");
      setAdding(false);
      games.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't add game", detail: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }

  async function remove(g: GameView) {
    try {
      await api.removeCustomGame(g.exes[0]);
      if (selected === g.id) setSelected(null);
      games.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't remove game", detail: errorMessage(e) });
    }
  }

  return (
    <>
      <div className="section-title">Your games</div>
      <div className="grid" style={{ gridTemplateColumns: current ? "minmax(0, 1fr) minmax(0, 1fr)" : "minmax(0, 1fr)" }}>
        <Panel
          title={`${list.length} game${list.length === 1 ? "" : "s"} found`}
          sub="Steam, Epic Games and GOG libraries, plus games you add"
          actions={
            <span className="row" style={{ gap: 6 }}>
              <button className="btn sm" onClick={() => setAdding((a) => !a)}><I.plus /> Add a game</button>
              <button className="btn ghost sm icon" disabled={games.loading} onClick={() => setRefreshTick((t) => t + 1)} aria-label="Scan again" title="Scan launchers again">
                {games.loading ? <Spinner /> : <I.refresh />}
              </button>
            </span>
          }
        >
          {adding && (
            <div className="field" style={{ marginBottom: 12 }}>
              <label htmlFor="gpath">Full path to the game's .exe</label>
              <div className="row">
                <input
                  id="gpath"
                  className="input mono"
                  style={{ flex: 1 }}
                  placeholder={String.raw`D:\Games\MyGame\MyGame.exe`}
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && add()}
                />
                <button className="btn primary" disabled={busy || !path.trim()} onClick={add}>{busy ? <Spinner /> : "Add"}</button>
              </div>
              <div className="hint">In File Explorer, Shift + right-click the .exe and choose "Copy as path", then paste it here.</div>
            </div>
          )}
          {games.error && <ErrorNote message={games.error} onRetry={games.reload} />}
          {games.loading && !games.data && <Loading label="Looking through your game launchers…" />}
          {games.data && list.length === 0 && (
            <Empty title="No games found">NOCTURN reads Steam, Epic Games and GOG Galaxy libraries. Add other games by their .exe path.</Empty>
          )}
          <div className="list">
            {list.map((g) => (
              <div
                key={g.id}
                className={`list-item ${selected === g.id ? "selected" : ""}`}
                style={{ alignItems: "center", cursor: "pointer", background: selected === g.id ? "var(--bg-3)" : undefined }}
                onClick={() => setSelected(selected === g.id ? null : g.id)}
              >
                <span className="status-dot" style={{ background: g.running ? "var(--good)" : "var(--bg-4)", boxShadow: g.running ? "0 0 8px rgba(12,163,12,.6)" : undefined }} />
                <div className="body">
                  <div className="title">{g.name}</div>
                  <div className="desc">
                    {g.launcher}
                    {g.sessions > 0 && ` · ${g.sessions} session${g.sessions === 1 ? "" : "s"}`}
                    {g.exes.length === 0 && " · executable not found"}
                  </div>
                </div>
                {g.running && <span className="tag good">Running</span>}
                {g.profile && <span className="tag solid">Profile</span>}
                {!g.id.startsWith("custom:") && (
                  <button
                    className="btn ghost sm"
                    title="Utilities and tools from a game store (e.g. Borderless Gaming, Wallpaper Engine) can be excluded"
                    onClick={(e) => {
                      e.stopPropagation();
                      markNotGame(g, true);
                    }}
                  >
                    Not a game
                  </button>
                )}
                {g.id.startsWith("custom:") && (
                  <button
                    className="btn ghost sm icon"
                    aria-label={`Remove ${g.name}`}
                    title="Remove from list (doesn't touch the game)"
                    onClick={(e) => {
                      e.stopPropagation();
                      remove(g);
                    }}
                  >
                    <I.x />
                  </button>
                )}
              </div>
            ))}
          </div>
          {hidden.length > 0 && (
            <div className="mt-1">
              <button className="btn ghost sm" onClick={() => setShowHidden((v) => !v)}>
                {showHidden ? "Hide" : "Show"} {hidden.length} marked as not a game
              </button>
              {showHidden && (
                <div className="list">
                  {hidden.map((g) => (
                    <div key={g.id} className="list-item" style={{ alignItems: "center" }}>
                      <div className="body">
                        <div className="title muted">{g.name}</div>
                        <div className="desc">{g.launcher} · not tracked as a game</div>
                      </div>
                      <button className="btn ghost sm" onClick={() => markNotGame(g, false)}>Restore</button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </Panel>
        {current && <GameProfileEditor game={current} onClose={() => setSelected(null)} onSaved={games.reload} />}
      </div>
      {!current && list.length > 0 && <p className="small dim">Select a game to set its priority, CPUs, graphics preference and fullscreen behaviour.</p>}
    </>
  );
}

// ---------------------------------------------------------- history

function SessionHistory({ version }: { version: number }) {
  const sessions = useLoad<SessionSummary[]>(() => api.gameSessions(), [version]);
  const [open, setOpen] = useState<number | null>(null);
  const rows = sessions.data ?? [];

  return (
    <section className="panel flush mt-2">
      <div className="panel-head">
        <div>
          <h2>Session history</h2>
          <div className="sub">Recorded automatically for sessions of a minute or more. Select a row for the verdict.</div>
        </div>
      </div>
      {sessions.error && <div style={{ padding: 16 }}><ErrorNote message={sessions.error} onRetry={sessions.reload} /></div>}
      {sessions.loading && !sessions.data && <Loading />}
      {sessions.data && rows.length === 0 && <Empty title="No sessions yet">Play a detected game with NOCTURN running and it will appear here.</Empty>}
      {rows.length > 0 && (
        <div className="table-wrap" style={{ maxHeight: 420 }}>
          <table className="table">
            <thead>
              <tr>
                <th>Game</th>
                <th>Started</th>
                <th className="r">Length</th>
                <th className="r">CPU avg</th>
                <th className="r">Busiest core</th>
                <th className="r">GPU avg</th>
                <th className="r">GPU peak °C</th>
                <th className="r">Game RAM peak</th>
                <th className="r">GPU-limited</th>
                <th className="r">CPU-limited</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((s, i) => {
                const key = s.id ?? i;
                return (
                  <SessionRow key={key} s={s} open={open === key} onToggle={() => setOpen(open === key ? null : key)} />
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function SessionRow({ s, open, onToggle }: { s: SessionSummary; open: boolean; onToggle: () => void }) {
  return (
    <>
      <tr onClick={onToggle} style={{ cursor: "pointer" }}>
        <td>{s.game_name}</td>
        <td className="small">{date(new Date(s.started_at).toISOString(), true)}</td>
        <td className="r num">{duration((s.ended_at - s.started_at) / 1000)}</td>
        <td className="r num">{s.avg_cpu.toFixed(0)}%</td>
        <td className="r num">{s.avg_max_core.toFixed(0)}%</td>
        <td className="r num">{s.avg_gpu != null ? `${s.avg_gpu.toFixed(0)}%` : "—"}</td>
        <td className="r num">{s.peak_gpu_temp != null ? s.peak_gpu_temp.toFixed(0) : "—"}</td>
        <td className="r num">{bytes(s.game_mem_peak)}</td>
        <td className="r num">{s.avg_gpu != null ? `${s.gpu_bound_pct.toFixed(0)}%` : "—"}</td>
        <td className="r num">{s.avg_gpu != null ? `${s.cpu_bound_pct.toFixed(0)}%` : "—"}</td>
      </tr>
      {open && (
        <tr>
          <td colSpan={10} className="small" style={{ whiteSpace: "normal" }}>
            {s.verdict}
          </td>
        </tr>
      )}
    </>
  );
}

// ---------------------------------------------------------- power plans

function PowerPlans() {
  const { dataVersion, toast, bumpData } = useApp();
  const plans = useLoad(() => api.powerPlans(), [dataVersion]);
  const [switching, setSwitching] = useState<string | null>(null);

  async function switchPlan(guid: string, name: string) {
    setSwitching(guid);
    try {
      const r = await api.setPowerPlan(guid);
      toast({
        tone: r.status === "completed" ? "good" : "warning",
        title: r.status === "completed" ? `Power plan set to ${name}` : "Power plan unchanged",
        detail: r.results[0]?.error ?? undefined,
        action: r.status === "completed" ? { label: "Undo", run: () => api.rollback(r.tx_id).then(() => bumpData()) } : undefined,
      });
      bumpData();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't change power plan", detail: errorMessage(e) });
    } finally {
      setSwitching(null);
    }
  }

  return (
    <>
      <div className="section-title">Power plan</div>
      <Panel sub="Switches are recorded in History and can be undone">
        {plans.loading && !plans.data && <Loading />}
        {plans.error && <ErrorNote message={plans.error} onRetry={plans.reload} />}
        <div className="list">
          {plans.data?.map((p) => (
            <div className="list-item" key={p.guid} style={{ alignItems: "center" }}>
              <span className="status-dot" style={{ background: p.active ? "var(--moon)" : "var(--bg-4)", boxShadow: p.active ? "0 0 8px rgba(var(--accent-rgb), .6)" : undefined }} />
              <div className="body">
                <div className="title">{p.name}</div>
                <div className="desc mono">{p.guid}</div>
              </div>
              {p.active ? (
                <span className="small">Active</span>
              ) : (
                <button className="btn sm" disabled={!!switching} onClick={() => switchPlan(p.guid, p.name)}>
                  {switching === p.guid ? <Spinner /> : "Use"}
                </button>
              )}
            </div>
          ))}
        </div>
      </Panel>
    </>
  );
}
