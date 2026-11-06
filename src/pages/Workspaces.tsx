// Workspaces: environments for different activities — their own look, apps,
// apps to open when switching, and optionally their own dashboard.

import { useState } from "react";
import { I } from "../components/Icons";
import { Empty, Note, PageHead, Panel, Seg, Toggle } from "../components/ui";
import { library, type LibraryItem } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { AppIcon } from "../library/tiles";
import { DashboardEditor } from "../shell/DashboardEditor";
import { usePrefs } from "../shell/PrefsProvider";
import { BACKGROUNDS, isHexColour, type BackgroundStyle, type WidgetSlot, type Workspace } from "../shell/prefs";
import { accentProblem, findTheme } from "../shell/theme";
import { useWorkspaces } from "../shell/WorkspaceProvider";

interface Template {
  key: string;
  name: string;
  glyph: string;
  description: string;
  accent: string | null;
  background: BackgroundStyle | null;
  categories: string[];
  widgets: WidgetSlot[];
}

const w = (type: WidgetSlot["type"], size: WidgetSlot["size"]): WidgetSlot => ({ id: `w-${type}-${Math.random().toString(36).slice(2, 7)}`, type, size });

const TEMPLATES: Template[] = [
  { key: "gaming", name: "Gaming", glyph: "🎮", description: "Game launchers, chat, music and performance at a glance.", accent: "#9fc3ff", background: "glow", categories: ["Games", "Communication"], widgets: [w("vitals", "l"), w("gaming", "m"), w("music", "m"), w("apps", "m"), w("quick", "m")] },
  { key: "study", name: "Study", glyph: "📚", description: "Notes, browser, calendar, focus music and the AI.", accent: "#a6e3cf", background: "horizon", categories: ["Productivity", "Browsers", "Websites"], widgets: [w("apps", "l"), w("music", "m"), w("ask", "m")] },
  { key: "creative", name: "Creative", glyph: "🎨", description: "Editing apps, music, AI and system monitoring.", accent: "#c8b6ff", background: "stars", categories: ["Creative", "Media"], widgets: [w("vitals", "l"), w("apps", "m"), w("music", "m"), w("storage", "s"), w("ask", "m")] },
  { key: "personal", name: "Personal", glyph: "🌙", description: "Messaging, entertainment and music.", accent: null, background: null, categories: ["Communication", "Media"], widgets: [w("apps", "l"), w("music", "m"), w("network", "s"), w("ask", "m")] },
  { key: "blank", name: "Blank", glyph: "✦", description: "Start from nothing.", accent: null, background: null, categories: [], widgets: [] },
];

const BG_LABEL: Record<BackgroundStyle, string> = { plain: "Plain", glow: "Moonglow", horizon: "Horizon", stars: "Starfield" };
const ACCENTS = ["#9fc3ff", "#a6e3cf", "#c8b6ff", "#f0c89a", "#f4a7b9", "#e8e08a", "#dfe2e8"];

function Editor({ ws, items, onChange, onDelete, onClose }: { ws: Workspace; items: LibraryItem[]; onChange: (w: Workspace) => void; onDelete: () => void; onClose: () => void }) {
  const { navigate } = useApp();
  const { prefs } = usePrefs();
  const theme = findTheme(prefs.appearance.theme, prefs.appearance.customThemes);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const set = (patch: Partial<Workspace>) => onChange({ ...ws, ...patch });
  const inWs = (id: string) => ws.apps.includes(id);
  return (
    <div className="stack" style={{ gap: 14 }}>
      <Panel
        title={`Edit ${ws.name}`}
        actions={
          <div className="row" style={{ gap: 6 }}>
            {confirmDelete ? (
              <>
                <button className="btn ghost sm" onClick={() => setConfirmDelete(false)}>Cancel</button>
                <button className="btn sm" onClick={onDelete}>Delete workspace</button>
              </>
            ) : (
              <button className="btn ghost sm" onClick={() => setConfirmDelete(true)}>Delete…</button>
            )}
            <button className="btn sm primary" onClick={onClose}>Done</button>
          </div>
        }
      >
        <div className="row" style={{ gap: 12, flexWrap: "wrap", alignItems: "flex-end" }}>
          <div className="field" style={{ width: 90 }}>
            <label htmlFor="wsg">Icon</label>
            <input id="wsg" className="input" maxLength={2} value={ws.glyph} onChange={(e) => set({ glyph: e.target.value })} />
          </div>
          <div className="field" style={{ flex: 1, minWidth: 180 }}>
            <label htmlFor="wsn">Name</label>
            <input id="wsn" className="input" maxLength={40} value={ws.name} onChange={(e) => set({ name: e.target.value })} />
          </div>
          <div className="field" style={{ flex: 2, minWidth: 240 }}>
            <label htmlFor="wsd">Description</label>
            <input id="wsd" className="input" maxLength={120} value={ws.description} onChange={(e) => set({ description: e.target.value })} />
          </div>
        </div>
        <div className="studio-row mt-2">
          <div><div className="studio-label">Accent while active</div><div className="small dim">Unreadable colours for your theme are greyed out.</div></div>
          <div className="studio-control row" style={{ gap: 8 }}>
            <button className={`chip ${ws.accent === null ? "on" : ""}`} onClick={() => set({ accent: null })}>Theme's</button>
            {ACCENTS.map((c) => (
              <button key={c} className={`swatch-btn ${ws.accent === c ? "on" : ""}`} style={{ background: c, width: 24, height: 24 }} disabled={!!accentProblem(c, theme.tokens)} aria-label={`Accent ${c}`} onClick={() => set({ accent: c })} />
            ))}
            <input type="color" aria-label="Custom accent" value={ws.accent && isHexColour(ws.accent) ? ws.accent : "#9fc3ff"} onChange={(e) => !accentProblem(e.target.value, theme.tokens) && set({ accent: e.target.value })} />
          </div>
        </div>
        <div className="studio-row">
          <div className="studio-label">Background while active</div>
          <div className="studio-control">
            <Seg<BackgroundStyle | "global"> value={ws.background ?? "global"} onChange={(v) => set({ background: v === "global" ? null : v })} options={[{ value: "global", label: "As usual" }, ...BACKGROUNDS.map((b) => ({ value: b, label: BG_LABEL[b] }))]} />
          </div>
        </div>
      </Panel>

      <Panel title="Apps" sub="Choose this workspace's apps, and which of them open when you switch to it.">
        {items.length === 0 ? (
          <Empty title="Your Library is empty">
            <button className="btn sm" onClick={() => navigate("library")}>Add apps to your Library <I.arrow /></button>
          </Empty>
        ) : (
          <div className="pick-list" style={{ maxHeight: 420 }}>
            {items.map((i) => (
              <div key={i.id} className="pick">
                <input type="checkbox" className="check" checked={inWs(i.id)} aria-label={`Include ${i.name}`} onChange={() => set(inWs(i.id) ? { apps: ws.apps.filter((x) => x !== i.id), launch: ws.launch.filter((x) => x !== i.id) } : { apps: [...ws.apps, i.id] })} />
                <AppIcon item={i} size={28} />
                <span className="truncate">{i.name}</span>
                <span className="small dim">{i.category}</span>
                <span className="row small" style={{ marginLeft: "auto", gap: 8, opacity: inWs(i.id) ? 1 : 0.35 }}>
                  Open when switching
                  <Toggle on={ws.launch.includes(i.id)} disabled={!inWs(i.id)} label={`Open ${i.name} when switching`} onChange={(v) => set({ launch: v ? [...ws.launch, i.id] : ws.launch.filter((x) => x !== i.id) })} />
                </span>
              </div>
            ))}
          </div>
        )}
      </Panel>

      <Panel title="Dashboard" sub="Give this workspace its own Home dashboard, or keep the main one.">
        <div className="row" style={{ gap: 10 }}>
          <Toggle on={ws.widgets !== null} label="Own dashboard" onChange={(v) => set({ widgets: v ? [...prefs.home.widgets] : null })} />
          <span className="small">{ws.widgets ? "This workspace has its own dashboard." : "Uses your main dashboard."}</span>
        </div>
      </Panel>
      {ws.widgets && <DashboardEditor title={`${ws.name}'s dashboard`} widgets={ws.widgets} onChange={(widgets) => set({ widgets })} />}
      <Note>Music for a workspace (for example a focus playlist when Study starts) arrives with Nocturn Music.</Note>
    </div>
  );
}

export function Workspaces() {
  const { prefs, update } = usePrefs();
  const { active, switchTo } = useWorkspaces();
  const items = useLoad(() => library.list(), []);
  const [editing, setEditing] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const list = prefs.workspaces.list;
  const editingWs = list.find((x) => x.id === editing) ?? null;
  const setList = (fn: (l: Workspace[]) => Workspace[]) => update((p) => ({ ...p, workspaces: { ...p.workspaces, list: fn(p.workspaces.list) } }));

  function create(t: Template) {
    const lib = items.data ?? [];
    const apps = lib.filter((i) => t.categories.includes(i.category)).slice(0, 12).map((i) => i.id);
    const ws: Workspace = {
      id: `ws-${Math.random().toString(36).slice(2, 10)}`,
      name: list.some((x) => x.name === t.name) ? `${t.name} 2` : t.key === "blank" ? "New workspace" : t.name,
      glyph: t.glyph,
      accent: t.accent,
      background: t.background,
      apps,
      launch: [],
      widgets: t.widgets.length ? t.widgets.map((x) => ({ ...x, id: `w-${x.type}-${Math.random().toString(36).slice(2, 8)}` })) : null,
      playlist: null,
      description: t.description,
    };
    setList((l) => [...l, ws]);
    setCreating(false);
    setEditing(ws.id);
  }

  return (
    <div className="page">
      <PageHead
        eyebrow="Workspaces"
        title="Environments for what you're doing"
        desc="Each workspace brings its own look, apps and dashboard. Switch from here or from the top of the window."
        actions={list.length < 20 && <button className="btn primary" onClick={() => { setCreating((v) => !v); setEditing(null); }}><I.plus /> New workspace</button>}
      />

      {creating && (
        <Panel title="Start from a template" sub="Apps are suggested from your Library by category; you can change everything afterwards.">
          <div className="grid cols-3" style={{ gap: 10 }}>
            {TEMPLATES.map((t) => (
              <button key={t.key} className="add-widget" onClick={() => create(t)}>
                <b>{t.glyph} {t.name}</b>
                <span className="small muted">{t.description}</span>
              </button>
            ))}
          </div>
        </Panel>
      )}

      {list.length === 0 && !creating ? (
        <div className="panel">
          <Empty title="No workspaces yet">Create one for gaming, studying, creative work or anything else.</Empty>
        </div>
      ) : (
        <div className="ws-grid mt-2">
          {list.map((x) => (
            <div key={x.id} className={`ws-card ${active?.id === x.id ? "on" : ""}`} style={x.accent ? ({ "--ws-accent": x.accent } as React.CSSProperties) : undefined}>
              <div className="row between">
                <span className="ws-big-glyph">{x.glyph || x.name.slice(0, 1)}</span>
                {active?.id === x.id && <span className="tag good">Active</span>}
              </div>
              <b className="mt-1" style={{ fontSize: 15 }}>{x.name}</b>
              <div className="small muted" style={{ flex: 1 }}>{x.description || "—"}</div>
              <div className="small dim">{x.apps.length} app{x.apps.length === 1 ? "" : "s"}{x.launch.length ? ` · opens ${x.launch.length}` : ""}{x.widgets ? " · own dashboard" : ""}</div>
              <div className="row mt-1" style={{ gap: 6 }}>
                {active?.id === x.id ? (
                  <button className="btn sm" onClick={() => switchTo(null)}>Leave</button>
                ) : (
                  <button className="btn sm primary" onClick={() => switchTo(x.id)}>Switch</button>
                )}
                <button className="btn ghost sm" onClick={() => setEditing(editing === x.id ? null : x.id)}>Edit</button>
              </div>
            </div>
          ))}
        </div>
      )}

      {editingWs && (
        <div className="mt-2">
          <Editor
            ws={editingWs}
            items={items.data ?? []}
            onChange={(ws) => setList((l) => l.map((y) => (y.id === ws.id ? ws : y)))}
            onDelete={() => {
              update((p) => ({ ...p, workspaces: { ...p.workspaces, list: p.workspaces.list.filter((y) => y.id !== editingWs.id), active: p.workspaces.active === editingWs.id ? null : p.workspaces.active } }));
              setEditing(null);
            }}
            onClose={() => setEditing(null)}
          />
        </div>
      )}

      {list.length > 0 && (
        <Panel className="mt-2" title="Behaviour">
          <div className="studio-row">
            <div className="studio-label">When Nocturn opens</div>
            <div className="studio-control">
              <select className="select" value={prefs.workspaces.startWith ?? ""} aria-label="Workspace when Nocturn opens" onChange={(e) => update((p) => ({ ...p, workspaces: { ...p.workspaces, startWith: e.target.value || null } }))}>
                <option value="">Stay in the last workspace</option>
                {list.map((x) => <option key={x.id} value={x.id}>Start in {x.name}</option>)}
              </select>
            </div>
          </div>
          <div className="studio-row">
            <div>
              <div className="studio-label">Ask before opening apps</div>
              <div className="small dim">Show which apps a workspace will open when you switch to it.</div>
            </div>
            <div className="studio-control">
              <Toggle on={prefs.workspaces.confirmLaunch} label="Ask before opening apps" onChange={(v) => update((p) => ({ ...p, workspaces: { ...p.workspaces, confirmLaunch: v } }))} />
            </div>
          </div>
        </Panel>
      )}
      <div className="mt-2">
        <Note>Workspaces organise and open apps; they don't move or resize other apps' windows. For window arrangements, Windows' Snap layouts and virtual desktops work alongside them. Starting Nocturn in a workspace applies its look only — apps open when you switch.</Note>
      </div>
    </div>
  );
}
