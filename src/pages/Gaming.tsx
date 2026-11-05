import { useState } from "react";
import { I } from "../components/Icons";
import { ProcessTable } from "../components/ProcessTable";
import { TweakList } from "../components/TweakList";
import { ErrorNote, Loading, PageHead, Panel, Spinner } from "../components/ui";
import { api, errorMessage, type TweakView } from "../lib/api";
import { useApp, useLoad } from "../lib/app";

const READINESS: { id: string; label: string; good: string; bad: string }[] = [
  { id: "gaming.game_mode", label: "Game Mode", good: "On", bad: "Off" },
  { id: "gaming.disable_background_recording", label: "Background recording", good: "Off", bad: "May be on" },
  { id: "gaming.power_high_performance", label: "High performance power plan", good: "Active", bad: "Not active" },
  { id: "gaming.hardware_gpu_scheduling", label: "Hardware GPU scheduling", good: "On", bad: "Off" },
  { id: "gaming.mouse_precision_off", label: "Mouse acceleration", good: "Off", bad: "On" },
];

export function Gaming() {
  const { requestApply, dataVersion, navigate, toast, bumpData } = useApp();
  const tweaks = useLoad(() => api.tweaks(), [dataVersion]);
  const profiles = useLoad(() => api.profiles(), []);
  const plans = useLoad(() => api.powerPlans(), [dataVersion]);
  const [switching, setSwitching] = useState<string | null>(null);

  const byId = new Map((tweaks.data ?? []).map((t) => [t.id, t] as [string, TweakView]));
  const gaming = profiles.data?.find((p) => p.id === "gaming");

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
    <div className="page">
      <PageHead
        eyebrow="Gaming"
        title="Gaming optimisation"
        desc="Check the Windows settings that affect games, see what's competing for resources in the background, and apply the Gaming profile with a full preview."
        actions={
          <button className="btn" onClick={() => navigate("assistant", { prompt: "Why am I getting low FPS or stutter in games? Please check my system." })}>
            <I.assistant /> Ask AI about FPS
          </button>
        }
      />

      <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)" }}>
        <Panel lit title="Gaming readiness" sub="Read live from Windows">
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
              className="btn primary mt-2"
              onClick={() =>
                requestApply({ tweakIds: gaming.tweaks, source: "profile:gaming", label: "Gaming profile", title: "Gaming profile", description: "Preview every change. Settings already configured are skipped; everything reversible can be rolled back." })
              }
            >
              Preview Gaming profile <I.arrow />
            </button>
          )}
          <p className="small dim mt-2">
            These settings remove obstacles; they don't guarantee higher FPS. Compare frame rates in the same scene before and after.
          </p>
        </Panel>

        <Panel title="Power plan" sub="Switches are recorded in History and can be undone">
          {plans.loading && !plans.data && <Loading />}
          {plans.error && <ErrorNote message={plans.error} onRetry={plans.reload} />}
          <div className="list">
            {plans.data?.map((p) => (
              <div className="list-item" key={p.guid} style={{ alignItems: "center" }}>
                <span className="status-dot" style={{ background: p.active ? "var(--moon)" : "var(--bg-4)", boxShadow: p.active ? "0 0 8px rgba(223,226,232,.6)" : undefined }} />
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
      </div>

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
