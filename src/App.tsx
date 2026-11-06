import { ApplyFlow } from "./components/ApplyFlow";
import { Sidebar, TitleBar } from "./components/Chrome";
import { AppProvider, useApp } from "./lib/app";
import { Assistant } from "./pages/Assistant";
import { Diagnostics } from "./pages/Diagnostics";
import { Gaming } from "./pages/Gaming";
import { Hardware } from "./pages/Hardware";
import { Processes } from "./pages/Processes";
import { History } from "./pages/History";
import { Network } from "./pages/Network";
import { Overview } from "./pages/Overview";
import { Settings } from "./pages/Settings";
import { Startup } from "./pages/Startup";
import { Storage } from "./pages/Storage";
import { Customise } from "./pages/Customise";
import { Services } from "./pages/Services";
import { Benchmarks } from "./pages/Benchmarks";
import { Maintenance } from "./pages/Maintenance";
import { AppProfiles } from "./pages/AppProfiles";
import { Tweak } from "./pages/Tweak";
import { Home } from "./pages/Home";
import { Studio } from "./pages/Studio";
import { Library } from "./pages/Library";
import { Workspaces } from "./pages/Workspaces";
import { ErrorBoundary } from "./shell/ErrorBoundary";
import { WorkspaceProvider } from "./shell/WorkspaceProvider";
import { MusicProvider } from "./music/MusicProvider";
import { Player } from "./music/Player";
import { Music } from "./pages/Music";
import { PrefsProvider, usePrefs } from "./shell/PrefsProvider";

function GuardedPages() {
  const { page, navigate } = useApp();
  return (
    <ErrorBoundary resetKey={page} onHome={() => navigate("home")}>
      <Pages />
    </ErrorBoundary>
  );
}

function Pages() {
  const { page } = useApp();
  switch (page) {
    case "home":
      return <Home />;
    case "library":
      return <Library />;
    case "music":
      return <Music />;
    case "workspaces":
      return <Workspaces />;
    case "studio":
      return <Studio />;
    case "overview":
      return <Overview />;
    case "hardware":
      return <Hardware />;
    case "benchmarks":
      return <Benchmarks />;
    case "processes":
      return <Processes />;
    case "tweak":
      return <Tweak />;
    case "customise":
      return <Customise />;
    case "profiles":
      return <AppProfiles />;
    case "gaming":
      return <Gaming />;
    case "diagnostics":
      return <Diagnostics />;
    case "network":
      return <Network />;
    case "storage":
      return <Storage />;
    case "services":
      return <Services />;
    case "maintenance":
      return <Maintenance />;
    case "startup":
      return <Startup />;
    case "assistant":
      return <Assistant />;
    case "history":
      return <History />;
    case "settings":
      return <Settings />;
  }
}

function Shell() {
  const { prefs } = usePrefs();
  const nav = prefs.navigation;
  return (
    <AppProvider>
      <MusicProvider>
      <WorkspaceProvider>
      <div className="shell">
        <TitleBar />
        <div className={["body", nav.sidebar === "right" ? "right" : "", nav.compact ? "compact" : ""].filter(Boolean).join(" ")}>
          <Sidebar />
          <main className="main" id="main" data-bg={prefs.workspaces.list.find((w) => w.id === prefs.workspaces.active)?.background ?? prefs.appearance.background}>
            <GuardedPages />
          </main>
        </div>
        <Player />
      </div>
      <ApplyFlow />
      </WorkspaceProvider>
      </MusicProvider>
    </AppProvider>
  );
}

export function App() {
  return (
    <PrefsProvider>
      <Shell />
    </PrefsProvider>
  );
}
