// Window chrome: custom title bar and the sidebar navigation.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import logo from "../logo.png";
import { api, type AppInfo } from "../lib/api";
import { useApp, type PageId } from "../lib/app";
import { pct } from "../lib/format";
import { useTelemetry } from "../lib/telemetry";
import { I } from "./Icons";

export function TitleBar() {
  const win = getCurrentWindow();
  const { latest } = useTelemetry();
  return (
    <div className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <img src={logo} alt="" />
        NOCTURN
      </div>
      <div className="center" data-tauri-drag-region onDoubleClick={() => win.toggleMaximize()} />
      {latest && (
        <div className="meta" data-tauri-drag-region>
          <span>CPU {pct(latest.cpu_usage)}</span>
          <span>RAM {pct((latest.mem_used / Math.max(1, latest.mem_total)) * 100)}</span>
        </div>
      )}
      <div className="win-controls">
        <button aria-label="Minimise" onClick={() => win.minimize()}>
          <I.minimize />
        </button>
        <button aria-label="Maximise" onClick={() => win.toggleMaximize()}>
          <I.maximize />
        </button>
        <button aria-label="Close" className="close" onClick={() => win.close()}>
          <I.close />
        </button>
      </div>
    </div>
  );
}

const NAV: { id: PageId; label: string; icon: keyof typeof I; group?: string }[] = [
  { id: "overview", label: "Overview", icon: "overview" },
  { id: "hardware", label: "Hardware", icon: "chip", group: "Monitor" },
  { id: "processes", label: "Processes", icon: "processes" },
  { id: "tweak", label: "NOCTURN Tweak", icon: "tweak", group: "Optimise" },
  { id: "gaming", label: "Gaming", icon: "gaming" },
  { id: "diagnostics", label: "System Diagnostics", icon: "diagnostics" },
  { id: "network", label: "Network", icon: "network" },
  { id: "startup", label: "Startup Manager", icon: "startup" },
  { id: "assistant", label: "AI Assistant", icon: "assistant", group: "Assist" },
  { id: "history", label: "History", icon: "history" },
  { id: "settings", label: "Settings", icon: "settings" },
];

export function Sidebar() {
  const { page, navigate } = useApp();
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo).catch(() => {});
  }, []);
  return (
    <nav className="sidebar" aria-label="Main">
      {NAV.map((n) => {
        const Icon = I[n.icon];
        return (
          <div key={n.id} style={{ display: "contents" }}>
            {n.group && <div className="group-label">{n.group}</div>}
            <button className={`nav-item ${page === n.id ? "active" : ""}`} aria-current={page === n.id ? "page" : undefined} onClick={() => navigate(n.id)}>
              <Icon />
              {n.label}
            </button>
          </div>
        );
      })}
      <div className="spacer" />
      <div className="footer">
        <span>NOCTURN {info ? `v${info.version}` : ""}</span>
        <span className="row" style={{ gap: 6 }}>
          <span className="status-dot" style={{ background: info?.is_elevated ? "var(--warning)" : "var(--ink-3)" }} />
          {info?.is_elevated ? "Running as administrator" : "Standard user · elevates on demand"}
        </span>
      </div>
    </nav>
  );
}
