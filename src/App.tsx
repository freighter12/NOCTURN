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
import { Tweak } from "./pages/Tweak";

function Pages() {
  const { page } = useApp();
  switch (page) {
    case "overview":
      return <Overview />;
    case "hardware":
      return <Hardware />;
    case "processes":
      return <Processes />;
    case "tweak":
      return <Tweak />;
    case "customise":
      return <Customise />;
    case "gaming":
      return <Gaming />;
    case "diagnostics":
      return <Diagnostics />;
    case "network":
      return <Network />;
    case "storage":
      return <Storage />;
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

export function App() {
  return (
    <AppProvider>
      <div className="shell">
        <TitleBar />
        <div className="body">
          <Sidebar />
          <main className="main" id="main">
            <Pages />
          </main>
        </div>
      </div>
      <ApplyFlow />
    </AppProvider>
  );
}
