import { ApplyFlow } from "./components/ApplyFlow";
import { Sidebar, TitleBar } from "./components/Chrome";
import { AppProvider, useApp } from "./lib/app";
import { Home } from "./pages/Home";
import { ErrorBoundary } from "./shell/ErrorBoundary";
import { WorkspaceProvider } from "./shell/WorkspaceProvider";
import { MusicProvider } from "./music/MusicProvider";
import { Player } from "./music/Player";
import { PrefsProvider, usePrefs } from "./shell/PrefsProvider";
import { SplashBridge } from "./shell/splash";
import { CommandPalette } from "./shell/CommandPalette";
import { lazy, Suspense } from "react";
import { Loading } from "./components/ui";

// Pages load the first time they're opened.
const Assistant = lazy(() => import("./pages/Assistant").then((m) => ({ default: m.Assistant })));
const Diagnostics = lazy(() => import("./pages/Diagnostics").then((m) => ({ default: m.Diagnostics })));
const Gaming = lazy(() => import("./pages/Gaming").then((m) => ({ default: m.Gaming })));
const Hardware = lazy(() => import("./pages/Hardware").then((m) => ({ default: m.Hardware })));
const Processes = lazy(() => import("./pages/Processes").then((m) => ({ default: m.Processes })));
const History = lazy(() => import("./pages/History").then((m) => ({ default: m.History })));
const Network = lazy(() => import("./pages/Network").then((m) => ({ default: m.Network })));
const Overview = lazy(() => import("./pages/Overview").then((m) => ({ default: m.Overview })));
const Settings = lazy(() => import("./pages/Settings").then((m) => ({ default: m.Settings })));
const Startup = lazy(() => import("./pages/Startup").then((m) => ({ default: m.Startup })));
const Storage = lazy(() => import("./pages/Storage").then((m) => ({ default: m.Storage })));
const Customise = lazy(() => import("./pages/Customise").then((m) => ({ default: m.Customise })));
const Services = lazy(() => import("./pages/Services").then((m) => ({ default: m.Services })));
const Benchmarks = lazy(() => import("./pages/Benchmarks").then((m) => ({ default: m.Benchmarks })));
const Maintenance = lazy(() => import("./pages/Maintenance").then((m) => ({ default: m.Maintenance })));
const AppProfiles = lazy(() => import("./pages/AppProfiles").then((m) => ({ default: m.AppProfiles })));
const Tweak = lazy(() => import("./pages/Tweak").then((m) => ({ default: m.Tweak })));
const Studio = lazy(() => import("./pages/Studio").then((m) => ({ default: m.Studio })));
const Library = lazy(() => import("./pages/Library").then((m) => ({ default: m.Library })));
const Workspaces = lazy(() => import("./pages/Workspaces").then((m) => ({ default: m.Workspaces })));
const Music = lazy(() => import("./pages/Music").then((m) => ({ default: m.Music })));

function GuardedPages() {
  const { page, navigate } = useApp();
  return (
    <ErrorBoundary resetKey={page} onHome={() => navigate("home")}>
      <Suspense fallback={<div className="page"><Loading label="Opening…" /></div>}>
        <Pages />
      </Suspense>
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
      <CommandPalette />
      <SplashBridge />
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
