// Home: the personal dashboard. Which widgets appear, their order and size
// come from preferences (edited in Customise › Dashboard).

import { I } from "../components/Icons";
import { PageHead } from "../components/ui";
import { useApp } from "../lib/app";
import { usePrefs } from "../shell/PrefsProvider";
import { widgetDef } from "../shell/widgets";

function greeting(d = new Date()) {
  const h = d.getHours();
  return h < 5 ? "Good night" : h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
}

export function Home() {
  const { prefs } = usePrefs();
  const { navigate } = useApp();
  const widgets = prefs.home.widgets;
  return (
    <div className="page">
      <PageHead
        eyebrow="Nocturn"
        title={greeting()}
        desc="Your PC at a glance. Choose what appears here in Customise."
        actions={
          <button className="btn ghost" onClick={() => navigate("studio")}>
            <I.grid /> Edit dashboard
          </button>
        }
      />
      {widgets.length === 0 ? (
        <div className="panel">
          <div className="small muted">Your dashboard is empty. Add widgets in Customise › Dashboard.</div>
        </div>
      ) : (
        <div className="widgets">
          {widgets.map((w) => (
            <div key={w.id} className={`widget-slot size-${w.size}`}>
              {widgetDef(w.type).render()}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
