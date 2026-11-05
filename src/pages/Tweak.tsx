import { useState } from "react";
import { I } from "../components/Icons";
import { TweakList } from "../components/TweakList";
import { ErrorNote, Loading, PageHead, Panel, Seg } from "../components/ui";
import { api, type Category } from "../lib/api";
import { useApp, useLoad } from "../lib/app";

type Tab = "all" | Category;

export function Tweak() {
  const { requestApply, dataVersion } = useApp();
  const tweaks = useLoad(() => api.tweaks(), [dataVersion]);
  const profiles = useLoad(() => api.profiles(), []);
  const [tab, setTab] = useState<Tab>("all");

  return (
    <div className="page">
      <PageHead
        eyebrow="NOCTURN Tweak"
        title="Optimisation engine"
        desc="Documented Windows settings, each shown with exactly what it changes. Originals are recorded before anything is modified, and every reversible change can be rolled back."
      />

      <div className="section-title" style={{ marginTop: 0 }}>Profiles</div>
      <div className="grid cols-4">
        {profiles.data?.map((p) => {
          const views = tweaks.data?.filter((t) => p.tweaks.includes(t.id)) ?? [];
          const done = views.filter((t) => t.status.state === "applied").length;
          const advanced = views.filter((t) => t.risk === "advanced").length;
          return (
            <Panel key={p.id} lit={p.id === "gaming"}>
              <div className="stack" style={{ gap: 8, height: "100%" }}>
                <div className="row between">
                  <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>{p.name}</h3>
                  <span className="dim small num">{p.tweaks.length} changes</span>
                </div>
                <div className="small muted" style={{ flex: 1 }}>
                  {p.description}
                </div>
                <div className="small dim">
                  {tweaks.data ? `${done} of ${p.tweaks.length} already applied` : "…"}
                  {advanced > 0 && ` · ${advanced} Advanced`}
                </div>
                <button
                  className={`btn ${p.id === "gaming" ? "primary" : ""}`}
                  onClick={() =>
                    requestApply({
                      tweakIds: p.tweaks,
                      source: `profile:${p.id}`,
                      label: `${p.name} profile`,
                      title: `${p.name} profile`,
                      description: "Review every change in this profile. Settings that are already configured are skipped.",
                    })
                  }
                >
                  Preview profile <I.arrow />
                </button>
              </div>
            </Panel>
          );
        })}
        <Panel>
          <div className="stack" style={{ gap: 8, height: "100%" }}>
            <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>Custom</h3>
            <div className="small muted" style={{ flex: 1 }}>
              Pick individual optimisations below. Every change is previewed before it's applied.
            </div>
            <button className="btn" onClick={() => document.getElementById("tweak-list")?.scrollIntoView({ behavior: "smooth" })}>
              Choose changes <I.arrow />
            </button>
          </div>
        </Panel>
      </div>

      <div className="row between" style={{ margin: "28px 0 10px" }} id="tweak-list">
        <div className="section-title" style={{ margin: 0 }}>All optimisations</div>
        <Seg<Tab>
          value={tab}
          onChange={setTab}
          options={[
            { value: "all", label: "All" },
            { value: "gaming", label: "Gaming" },
            { value: "performance", label: "Performance" },
            { value: "network", label: "Network" },
            { value: "privacy", label: "Privacy & control" },
          ]}
        />
      </div>
      <section className="panel flush">
        {tweaks.loading && !tweaks.data && <Loading label="Checking the current state of each setting…" />}
        {tweaks.error && <div style={{ padding: 16 }}><ErrorNote message={tweaks.error} onRetry={tweaks.reload} /></div>}
        {tweaks.data && <TweakList tweaks={tweaks.data} category={tab === "all" ? undefined : tab} />}
      </section>
      <p className="small dim mt-2">
        NOCTURN never disables Microsoft Defender, Windows Update, the firewall, UAC or core Windows services, and doesn't promise FPS gains it hasn't measured.
      </p>
    </div>
  );
}
