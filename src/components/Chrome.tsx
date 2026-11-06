// Window chrome: custom title bar and the sidebar navigation.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import logo from "../logo.png";
import { api, type AppInfo } from "../lib/api";
import { useApp, type PageId } from "../lib/app";
import { ALL_PAGES, SECTIONS, sectionOf, type NavSection } from "../shell/nav";
import { usePrefs } from "../shell/PrefsProvider";
import { useWorkspaces } from "../shell/WorkspaceProvider";
import { pct } from "../lib/format";
import { useTelemetry } from "../lib/telemetry";
import { I } from "./Icons";

/** Title-bar menu for switching workspaces quickly. */
function WorkspaceSwitcher() {
  const { prefs } = usePrefs();
  const { active, switchTo } = useWorkspaces();
  const { navigate } = useApp();
  const [open, setOpen] = useState(false);
  const list = prefs.workspaces.list;
  if (list.length === 0) return null;
  const pick = (id: string | null) => {
    setOpen(false);
    if (id !== (active?.id ?? null)) switchTo(id);
  };
  return (
    <div className="ws-switch">
      <button className="ws-current" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <span className="ws-glyph">{active?.glyph || "◐"}</span>
        {active ? active.name : "No workspace"}
        <span className="chev">▾</span>
      </button>
      {open && (
        <>
          <div className="ws-backdrop" onClick={() => setOpen(false)} />
          <div className="ws-menu" role="menu">
            {list.map((w) => (
              <button key={w.id} role="menuitemradio" aria-checked={active?.id === w.id} className={active?.id === w.id ? "on" : ""} onClick={() => pick(w.id)}>
                <span className="ws-glyph">{w.glyph || w.name.slice(0, 1)}</span>
                {w.name}
              </button>
            ))}
            <button role="menuitemradio" aria-checked={!active} className={!active ? "on" : ""} onClick={() => pick(null)}>
              <span className="ws-glyph">◐</span>No workspace
            </button>
            <div className="ws-sep" />
            <button role="menuitem" onClick={() => { setOpen(false); navigate("workspaces"); }}>Manage workspaces…</button>
          </div>
        </>
      )}
    </div>
  );
}

export function TitleBar() {
  const win = getCurrentWindow();
  const { latest } = useTelemetry();
  return (
    <div className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <img src={logo} alt="" />
        NOCTURN
      </div>
      <div className="center" data-tauri-drag-region onDoubleClick={() => win.toggleMaximize()}>
        <WorkspaceSwitcher />
      </div>
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

export function Sidebar() {
  const { page, navigate } = useApp();
  const { prefs, update } = usePrefs();
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo).catch(() => {});
  }, []);
  const nav = prefs.navigation;
  const sections = nav.order.map((id) => SECTIONS.find((s) => s.id === id)).filter((s): s is NavSection => !!s && !nav.hidden.includes(s.id));
  const current = sectionOf(page);
  const toggle = (id: string) =>
    update((p) => ({ ...p, navigation: { ...p.navigation, expanded: p.navigation.expanded.includes(id) ? p.navigation.expanded.filter((x) => x !== id) : [...p.navigation.expanded, id] } }));

  const item = (id: PageId, label: string, icon: keyof typeof I, sub = false) => {
    const Icon = I[icon];
    return (
      <button key={`${sub ? "c" : "p"}-${id}`} className={`nav-item ${sub ? "sub" : ""} ${page === id ? "active" : ""}`} aria-current={page === id ? "page" : undefined} title={nav.compact ? label : undefined} onClick={() => navigate(id)}>
        <Icon />
        <span className="label">{label}</span>
      </button>
    );
  };

  return (
    <nav className="sidebar" aria-label="Main">
      {nav.pinned.length > 0 && (
        <>
          <div className="group-label">Pinned</div>
          {nav.pinned.map((id) => {
            const p = ALL_PAGES.find((x) => x.id === id);
            return p ? item(p.id, p.label, p.icon) : null;
          })}
          <div className="group-label">Nocturn</div>
        </>
      )}
      {sections.map((s) => {
        if (!s.children) return item(s.page!, s.label, s.icon);
        const Icon = I[s.icon];
        const open = nav.expanded.includes(s.id);
        const here = current?.id === s.id;
        return (
          <div key={s.id} style={{ display: "contents" }}>
            <button
              className={`nav-item section ${here && !open ? "active" : ""} ${here ? "here" : ""}`}
              aria-expanded={open}
              title={nav.compact ? s.label : undefined}
              onClick={() => {
                toggle(s.id);
                if (!open && !here) navigate(s.children![0].id);
              }}
            >
              <Icon />
              <span className="label">{s.label}</span>
              <span className="chev" aria-hidden>{open ? "▾" : "▸"}</span>
            </button>
            {open && s.children.map((c) => item(c.id, c.label, c.icon, true))}
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
