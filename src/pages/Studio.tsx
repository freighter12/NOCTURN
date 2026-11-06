// Experience Studio: customise how Nocturn looks and is laid out. Every
// control writes to the central preference document, which applies live.

import { useMemo, useRef, useState, type ReactNode } from "react";
import { I } from "../components/Icons";
import { Note, PageHead, Panel, Seg, Toggle } from "../components/ui";
import { useApp, type PageId } from "../lib/app";
import { ALL_PAGES, SECTIONS } from "../shell/nav";
import { usePrefs } from "../shell/PrefsProvider";
import { BACKGROUNDS, LOCKED_SECTIONS, sanitizeTheme, type BackgroundStyle, type Prefs, } from "../shell/prefs";
import { OFFICIAL_THEMES, accentProblem, findTheme, readabilityProblem, type Theme, type ThemeTokens } from "../shell/theme";
import { DashboardEditor, move } from "../shell/DashboardEditor";

type Tab = "appearance" | "themes" | "layout" | "dashboard" | "music" | "accessibility";

const ACCENTS = ["#dfe2e8", "#b9cdf5", "#9fc3ff", "#a6e3cf", "#c8b6ff", "#f0c89a", "#f4a7b9", "#e8e08a"];
/** Deeper presets that stay readable on light themes. */
const ACCENTS_LIGHT = ["#2d3644", "#1f4e9c", "#1d6b57", "#6b3fa0", "#9a4a12", "#a3264a", "#5b6b12", "#0e6a86"];
const BG_LABEL: Record<BackgroundStyle, string> = { plain: "Plain", glow: "Moonglow", horizon: "Horizon", stars: "Starfield" };
const TOKEN_LABELS: [keyof ThemeTokens, string][] = [
  ["bg0", "Window"], ["bg1", "Sidebar"], ["bg2", "Panels"], ["bg3", "Raised / hover"], ["bg4", "Inputs"],
  ["ink0", "Main text"], ["ink1", "Secondary text"], ["ink2", "Muted text"], ["ink3", "Faint text"],
  ["accent", "Accent"], ["accent2", "Accent (dim)"],
];

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="studio-row">
      <div>
        <div className="studio-label">{label}</div>
        {hint && <div className="small dim">{hint}</div>}
      </div>
      <div className="studio-control">{children}</div>
    </div>
  );
}

function Slider({ value, min, max, step, onChange, label, format }: { value: number; min: number; max: number; step: number; onChange: (v: number) => void; label: string; format: (v: number) => string }) {
  return (
    <div className="row" style={{ gap: 10 }}>
      <input type="range" className="range" min={min} max={max} step={step} value={value} aria-label={label} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="small num" style={{ width: 46, textAlign: "right" }}>{format(value)}</span>
    </div>
  );
}

/** A miniature window painted with a theme's colours. */
function ThemePreview({ t }: { t: ThemeTokens }) {
  return (
    <div className="theme-preview" style={{ background: t.bg0, borderColor: t.bg4 }}>
      <div style={{ background: t.bg1, width: "26%" }}>
        <i style={{ background: t.accent, width: "60%" }} />
        <i style={{ background: t.ink3, width: "45%" }} />
        <i style={{ background: t.ink3, width: "52%" }} />
      </div>
      <div style={{ flex: 1, padding: 6, display: "grid", gap: 5 }}>
        <div style={{ background: t.bg2, borderRadius: 3, padding: 5, display: "grid", gap: 3 }}>
          <i style={{ background: t.ink0, width: "55%" }} />
          <i style={{ background: t.ink2, width: "80%" }} />
        </div>
        <div style={{ background: t.bg2, borderRadius: 3, padding: 5 }}>
          <i style={{ background: t.accent, width: "35%", height: 5 }} />
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------- appearance

function Appearance({ prefs, set }: { prefs: Prefs; set: (fn: (a: Prefs["appearance"]) => Prefs["appearance"]) => void }) {
  const a = prefs.appearance;
  const theme = findTheme(a.theme, a.customThemes);
  const [custom, setCustom] = useState(a.accent ?? theme.tokens.accent);
  const problem = accentProblem(custom, theme.tokens);
  return (
    <div className="stack" style={{ gap: 14 }}>
      <Panel title="Accent colour" sub={`Used for highlights, the active page and focus. ${theme.name}'s own accent is the first swatch.`}>
        <div className="row" style={{ gap: 10, flexWrap: "wrap" }}>
          {[theme.tokens.accent, ...(theme.kind === "light" ? ACCENTS_LIGHT : ACCENTS).filter((c) => c !== theme.tokens.accent)].map((c, i) => {
            const bad = accentProblem(c, theme.tokens);
            const on = (a.accent ?? theme.tokens.accent) === c;
            return (
              <button
                key={c}
                className={`swatch-btn ${on ? "on" : ""}`}
                style={{ background: c }}
                disabled={!!bad}
                title={bad ?? (i === 0 ? `${theme.name} default` : c)}
                aria-label={i === 0 ? "Theme default accent" : `Accent ${c}`}
                onClick={() => set((x) => ({ ...x, accent: i === 0 ? null : c }))}
              />
            );
          })}
          <label className="row small" style={{ gap: 8, marginLeft: 8 }}>
            Custom
            <input type="color" value={custom} aria-label="Custom accent colour" onChange={(e) => setCustom(e.target.value)} />
            <button className="btn sm" disabled={!!problem} onClick={() => set((x) => ({ ...x, accent: custom }))}>Use</button>
          </label>
        </div>
        {problem && <div className="small mt-1" style={{ color: "var(--ink-warning)" }}>{problem}</div>}
      </Panel>

      <Panel title="Surfaces">
        <Row label="Corner roundness" hint="Square to very round.">
          <Slider label="Corner roundness" value={a.radius} min={0} max={2} step={0.25} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => set((x) => ({ ...x, radius: v }))} />
        </Row>
        <Row label="Panel transparency" hint="Lets the page background show through panels.">
          <Slider label="Panel transparency" value={a.transparency} min={0} max={0.6} step={0.05} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => set((x) => ({ ...x, transparency: v }))} />
        </Row>
        <Row label="Blur behind panels" hint={a.transparency === 0 ? "Needs some transparency to have an effect." : "Softens what shows through. Uses a little more GPU."}>
          <Slider label="Blur" value={a.blur} min={0} max={24} step={2} format={(v) => `${v}px`} onChange={(v) => set((x) => ({ ...x, blur: v }))} />
        </Row>
        <Row label="Shadows"><Toggle on={a.shadows} label="Panel shadows" onChange={(v) => set((x) => ({ ...x, shadows: v }))} /></Row>
        <Row label="Borders"><Toggle on={a.borders} label="Panel borders" onChange={(v) => set((x) => ({ ...x, borders: v }))} /></Row>
        <Row label="High contrast" hint="Brighter text and stronger borders.">
          <Toggle on={a.highContrast} label="High contrast" onChange={(v) => set((x) => ({ ...x, highContrast: v }))} />
        </Row>
        <Row label="Page background">
          <Seg<BackgroundStyle> value={a.background} onChange={(v) => set((x) => ({ ...x, background: v }))} options={BACKGROUNDS.map((b) => ({ value: b, label: BG_LABEL[b] }))} />
        </Row>
      </Panel>
    </div>
  );
}

// ----------------------------------------------------------------- themes

function newId() {
  return `custom-${Math.random().toString(36).slice(2, 10)}`;
}

function Themes({ prefs, update }: { prefs: Prefs; update: (fn: (p: Prefs) => Prefs) => void }) {
  const { toast } = useApp();
  const a = prefs.appearance;
  const [draft, setDraft] = useState<Theme | null>(null);
  const [importText, setImportText] = useState("");
  const [showImport, setShowImport] = useState(false);
  const problem = draft ? readabilityProblem(draft.tokens) : null;
  const setTheme = (id: string) => update((p) => ({ ...p, appearance: { ...p.appearance, theme: id } }));

  function save() {
    if (!draft || problem) return;
    update((p) => {
      const list = p.appearance.customThemes.filter((t) => t.id !== draft.id);
      return { ...p, appearance: { ...p.appearance, customThemes: [...list, draft], theme: draft.id } };
    });
    toast({ tone: "good", title: `Theme "${draft.name}" saved` });
    setDraft(null);
  }

  function remove(t: Theme) {
    update((p) => ({ ...p, appearance: { ...p.appearance, customThemes: p.appearance.customThemes.filter((x) => x.id !== t.id), theme: p.appearance.theme === t.id ? "nocturn" : p.appearance.theme } }));
  }

  async function exportTheme(t: Theme) {
    const text = JSON.stringify({ nocturnTheme: 1, name: t.name, kind: t.kind, tokens: t.tokens });
    try {
      await navigator.clipboard.writeText(text);
      toast({ tone: "good", title: "Theme copied", detail: "Paste it into Import on any Nocturn to use it." });
    } catch {
      setImportText(text);
      setShowImport(true);
    }
  }

  function importTheme() {
    try {
      const raw = JSON.parse(importText);
      const t = sanitizeTheme({ ...raw, id: newId(), description: "Imported" });
      if (!t) {
        const why = raw?.tokens ? readabilityProblem(raw.tokens) : null;
        throw new Error(why ?? "That isn't a valid Nocturn theme.");
      }
      update((p) => ({ ...p, appearance: { ...p.appearance, customThemes: [...p.appearance.customThemes, t], theme: t.id } }));
      toast({ tone: "good", title: `Imported "${t.name}"` });
      setImportText("");
      setShowImport(false);
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't import", detail: e instanceof Error ? e.message : String(e) });
    }
  }

  const card = (t: Theme, custom: boolean) => (
    <div key={t.id} className={`theme-card ${a.theme === t.id ? "on" : ""}`}>
      <button className="theme-pick" onClick={() => setTheme(t.id)} aria-pressed={a.theme === t.id}>
        <ThemePreview t={t.tokens} />
        <div className="row between" style={{ marginTop: 8 }}>
          <b style={{ fontWeight: 600 }}>{t.name}</b>
          <span className="small dim">{t.kind === "light" ? "Light" : "Dark"}</span>
        </div>
        <div className="small muted">{t.description || "Custom theme"}</div>
      </button>
      <div className="row" style={{ gap: 4, marginTop: 6 }}>
        <button className="btn ghost sm" onClick={() => setDraft({ ...t, id: custom ? t.id : newId(), name: custom ? t.name : `${t.name} (copy)`, description: custom ? t.description : `Based on ${t.name}` })}>{custom ? "Edit" : "Customise"}</button>
        <button className="btn ghost sm" onClick={() => exportTheme(t)}>Export</button>
        {custom && <button className="btn ghost sm" onClick={() => remove(t)}>Delete</button>}
      </div>
    </div>
  );

  return (
    <div className="stack" style={{ gap: 14 }}>
      {draft && (
        <Panel title={a.customThemes.some((t) => t.id === draft.id) ? `Edit ${draft.name}` : "New theme"} sub="Pick each colour. Nocturn checks that text stays readable before saving.">
          <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1fr) 260px", gap: 18 }}>
            <div className="stack" style={{ gap: 10 }}>
              <div className="row" style={{ gap: 10 }}>
                <input className="input" style={{ flex: 1 }} maxLength={40} value={draft.name} aria-label="Theme name" onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
                <Seg<"dark" | "light"> value={draft.kind} onChange={(v) => setDraft({ ...draft, kind: v })} options={[{ value: "dark", label: "Dark" }, { value: "light", label: "Light" }]} />
              </div>
              <div className="token-grid">
                {TOKEN_LABELS.map(([k, label]) => (
                  <label key={k} className="token">
                    <input type="color" value={draft.tokens[k]} aria-label={label} onChange={(e) => setDraft({ ...draft, tokens: { ...draft.tokens, [k]: e.target.value } })} />
                    <span className="small">{label}</span>
                    <span className="mono dim small">{draft.tokens[k]}</span>
                  </label>
                ))}
              </div>
            </div>
            <div>
              <ThemePreview t={draft.tokens} />
              {problem ? (
                <div className="small mt-1" style={{ color: "var(--ink-warning)" }}>{problem}</div>
              ) : (
                <div className="small mt-1" style={{ color: "var(--ink-good)" }}>Text is readable on these colours.</div>
              )}
            </div>
          </div>
          <div className="row mt-2" style={{ justifyContent: "flex-end", gap: 8 }}>
            <button className="btn ghost" onClick={() => setDraft(null)}>Cancel</button>
            <button className="btn primary" disabled={!!problem || !draft.name.trim()} onClick={save}>Save and use</button>
          </div>
        </Panel>
      )}

      <div className="section-title" style={{ margin: 0 }}>Official themes</div>
      <div className="theme-grid">{OFFICIAL_THEMES.map((t) => card(t, false))}</div>

      <div className="row between">
        <div className="section-title" style={{ margin: 0 }}>Your themes</div>
        <div className="row" style={{ gap: 6 }}>
          <button className="btn sm" onClick={() => setShowImport((v) => !v)}>Import</button>
          <button className="btn sm primary" onClick={() => { const base = findTheme(a.theme, a.customThemes); setDraft({ ...base, id: newId(), name: "My theme", description: `Based on ${base.name}` }); }}>
            <I.plus /> New theme
          </button>
        </div>
      </div>
      {showImport && (
        <Panel>
          <textarea className="textarea mono" rows={3} placeholder='Paste a theme, e.g. {"nocturnTheme":1,"name":"…","tokens":{…}}' value={importText} onChange={(e) => setImportText(e.target.value)} aria-label="Theme to import" />
          <div className="row mt-1" style={{ justifyContent: "flex-end" }}>
            <button className="btn primary sm" disabled={!importText.trim()} onClick={importTheme}>Import theme</button>
          </div>
        </Panel>
      )}
      {a.customThemes.length === 0 ? <div className="small dim">No custom themes yet. Customise an official one or start a new theme.</div> : <div className="theme-grid">{a.customThemes.map((t) => card(t, true))}</div>}
    </div>
  );
}

// ----------------------------------------------------------------- layout

function Layout({ prefs, set }: { prefs: Prefs; set: (fn: (n: Prefs["navigation"]) => Prefs["navigation"]) => void }) {
  const n = prefs.navigation;
  const drag = useRef<number | null>(null);
  const [pin, setPin] = useState<PageId | "">("");
  return (
    <div className="stack" style={{ gap: 14 }}>
      <Panel title="Sidebar">
        <Row label="Position">
          <Seg<"left" | "right"> value={n.sidebar} onChange={(v) => set((x) => ({ ...x, sidebar: v }))} options={[{ value: "left", label: "Left" }, { value: "right", label: "Right" }]} />
        </Row>
        <Row label="Compact" hint="Icons only; hover an icon to see its name.">
          <Toggle on={n.compact} label="Compact sidebar" onChange={(v) => set((x) => ({ ...x, compact: v }))} />
        </Row>
        <Row label="Open Nocturn on">
          <select className="select" value={n.startPage} aria-label="Start page" onChange={(e) => set((x) => ({ ...x, startPage: e.target.value as PageId }))}>
            {ALL_PAGES.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
          </select>
        </Row>
      </Panel>

      <Panel title="Sections" sub="Drag to reorder, or use the arrows. System and Settings always stay visible so you can find your way back.">
        <div className="list">
          {n.order.map((id, i) => {
            const s = SECTIONS.find((x) => x.id === id);
            if (!s) return null;
            const Icon = I[s.icon];
            const locked = LOCKED_SECTIONS.includes(id);
            const hidden = n.hidden.includes(id);
            return (
              <div
                key={id}
                className="list-item draggable"
                draggable
                onDragStart={() => (drag.current = i)}
                onDragOver={(e) => e.preventDefault()}
                onDrop={() => {
                  if (drag.current != null) set((x) => ({ ...x, order: move(x.order, drag.current!, i) }));
                  drag.current = null;
                }}
                style={{ alignItems: "center", opacity: hidden ? 0.5 : 1 }}
              >
                <span className="drag-handle" aria-hidden>⋮⋮</span>
                <Icon />
                <div className="body">
                  <div className="title">{s.label}</div>
                  <div className="desc">{s.description}</div>
                </div>
                <button className="btn ghost sm icon" aria-label={`Move ${s.label} up`} disabled={i === 0} onClick={() => set((x) => ({ ...x, order: move(x.order, i, i - 1) }))}>↑</button>
                <button className="btn ghost sm icon" aria-label={`Move ${s.label} down`} disabled={i === n.order.length - 1} onClick={() => set((x) => ({ ...x, order: move(x.order, i, i + 1) }))}>↓</button>
                {locked ? (
                  <span className="tag" title="Always visible"><I.lock /> Always shown</span>
                ) : (
                  <Toggle on={!hidden} label={`Show ${s.label}`} onChange={(v) => set((x) => ({ ...x, hidden: v ? x.hidden.filter((h) => h !== id) : [...x.hidden, id] }))} />
                )}
              </div>
            );
          })}
        </div>
      </Panel>

      <Panel title="Pinned pages" sub="Shortcuts shown at the top of the sidebar.">
        <div className="chips">
          {n.pinned.map((id) => (
            <button key={id} className="chip" onClick={() => set((x) => ({ ...x, pinned: x.pinned.filter((p) => p !== id) }))} title="Unpin">
              {ALL_PAGES.find((p) => p.id === id)?.label ?? id} ✕
            </button>
          ))}
          {n.pinned.length === 0 && <span className="small dim">Nothing pinned.</span>}
        </div>
        <div className="row mt-2" style={{ gap: 8 }}>
          <select className="select" value={pin} aria-label="Page to pin" onChange={(e) => setPin(e.target.value as PageId)}>
            <option value="">Choose a page…</option>
            {ALL_PAGES.filter((p) => !n.pinned.includes(p.id)).map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
          </select>
          <button className="btn sm" disabled={!pin || n.pinned.length >= 12} onClick={() => { if (pin) set((x) => ({ ...x, pinned: [...x.pinned, pin] })); setPin(""); }}>
            <I.pin /> Pin
          </button>
        </div>
      </Panel>
    </div>
  );
}

// -------------------------------------------------------------- dashboard

function Dashboard({ prefs, update }: { prefs: Prefs; update: (fn: (p: Prefs) => Prefs) => void }) {
  const { navigate } = useApp();
  return (
    <DashboardEditor
      widgets={prefs.home.widgets}
      onChange={(w) => update((p) => ({ ...p, home: { widgets: w } }))}
      actions={<button className="btn ghost sm" onClick={() => navigate("home")}>View Home <I.arrow /></button>}
    />
  );
}

// ------------------------------------------------------------------ music

const MODES: { value: Prefs["music"]["playerMode"]; label: string; hint: string }[] = [
  { value: "full", label: "Full", hint: "Large artwork, lyrics, queue and visualiser over the whole window." },
  { value: "standard", label: "Standard", hint: "A bar along the bottom of the window." },
  { value: "mini", label: "Mini", hint: "A small floating card in the corner." },
  { value: "minimal", label: "Minimal", hint: "Just play/pause and the title, in a pill." },
];

function MusicPrefs({ prefs, set }: { prefs: Prefs; set: (fn: (m: Prefs["music"]) => Prefs["music"]) => void }) {
  const mm = prefs.music;
  return (
    <Panel title="Player" sub="How Nocturn Music looks while it plays. You can also switch modes from the player itself.">
      <Row label="Player mode" hint={MODES.find((x) => x.value === mm.playerMode)?.hint}>
        <Seg value={mm.playerMode} onChange={(v) => set((x) => ({ ...x, playerMode: v }))} options={MODES.map(({ value, label }) => ({ value, label }))} />
      </Row>
      <Row label="Visualiser" hint="Animated bars in the full player. Static when reduce motion is on.">
        <Toggle on={mm.visualiser} label="Visualiser" onChange={(v) => set((x) => ({ ...x, visualiser: v }))} />
      </Row>
      <Row label="Lyrics" hint="Show lyrics from the file or a matching .lrc file in the full player.">
        <Toggle on={mm.showLyrics} label="Lyrics" onChange={(v) => set((x) => ({ ...x, showLyrics: v }))} />
      </Row>
    </Panel>
  );
}

// ---------------------------------------------------------- accessibility

function Accessibility({ prefs, set }: { prefs: Prefs; set: (fn: (x: Prefs["accessibility"]) => Prefs["accessibility"]) => void }) {
  const { settings } = useApp();
  const x = prefs.accessibility;
  return (
    <Panel title="Comfort">
      <Row label="Reduce motion" hint="Turns off page transitions and animated charts.">
        <Toggle on={x.reduceMotion || !!settings?.reduce_motion} label="Reduce motion" onChange={(v) => set((p) => ({ ...p, reduceMotion: v }))} />
      </Row>
      <Row label="Text size">
        <Slider label="Text size" value={x.fontScale} min={0.85} max={1.4} step={0.05} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => set((p) => ({ ...p, fontScale: v }))} />
      </Row>
      <Row label="Interface size" hint="Scales everything, like zooming.">
        <Slider label="Interface size" value={x.uiScale} min={0.8} max={1.5} step={0.05} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => set((p) => ({ ...p, uiScale: v }))} />
      </Row>
      {settings?.reduce_motion && <div className="small dim">Reduce motion is also switched on in Settings.</div>}
    </Panel>
  );
}

// ------------------------------------------------------------------- page

export function Studio() {
  const { prefs, update, reset } = usePrefs();
  const [tab, setTab] = useState<Tab>("appearance");
  const [confirmAll, setConfirmAll] = useState(false);
  const theme = useMemo(() => findTheme(prefs.appearance.theme, prefs.appearance.customThemes), [prefs.appearance.theme, prefs.appearance.customThemes]);
  const resetFor: Record<Tab, () => void> = {
    appearance: () => update((p) => ({ ...p, appearance: { ...p.appearance, accent: null, radius: 1, transparency: 0, blur: 0, shadows: true, borders: true, highContrast: false, background: "plain" } })),
    themes: () => update((p) => ({ ...p, appearance: { ...p.appearance, theme: "nocturn" } })),
    layout: () => reset("navigation"),
    dashboard: () => reset("home"),
    music: () => update((p) => ({ ...p, music: { ...p.music, playerMode: "standard", visualiser: true, showLyrics: true } })),
    accessibility: () => reset("accessibility"),
  };
  return (
    <div className="page">
      <PageHead
        eyebrow="Experience Studio"
        title="Make Nocturn yours"
        desc={`Changes apply as you make them and are remembered. Current theme: ${theme.name}.`}
        actions={
          confirmAll ? (
            <span className="row" style={{ gap: 6 }}>
              <button className="btn ghost" onClick={() => setConfirmAll(false)}>Cancel</button>
              <button className="btn primary" onClick={() => { reset(); setConfirmAll(false); }}>Reset everything</button>
            </span>
          ) : (
            <button className="btn ghost" onClick={() => setConfirmAll(true)}>Reset everything…</button>
          )
        }
      />
      <div className="row between" style={{ marginBottom: 14 }}>
        <Seg<Tab>
          value={tab}
          onChange={setTab}
          options={[
            { value: "appearance", label: "Appearance" },
            { value: "themes", label: "Themes" },
            { value: "layout", label: "Layout" },
            { value: "dashboard", label: "Dashboard" },
            { value: "music", label: "Music" },
            { value: "accessibility", label: "Accessibility" },
          ]}
        />
        <button className="btn ghost sm" onClick={resetFor[tab]}>Reset this section</button>
      </div>
      {tab === "appearance" && <Appearance prefs={prefs} set={(fn) => update((p) => ({ ...p, appearance: fn(p.appearance) }))} />}
      {tab === "themes" && <Themes prefs={prefs} update={update} />}
      {tab === "layout" && <Layout prefs={prefs} set={(fn) => update((p) => ({ ...p, navigation: fn(p.navigation) }))} />}
      {tab === "dashboard" && <Dashboard prefs={prefs} update={update} />}
      {tab === "music" && <MusicPrefs prefs={prefs} set={(fn) => update((p) => ({ ...p, music: fn(p.music) }))} />}
      {tab === "accessibility" && <Accessibility prefs={prefs} set={(fn) => update((p) => ({ ...p, accessibility: fn(p.accessibility) }))} />}
      <div className="mt-2">
        <Note>Windows' own taskbar, File Explorer and notification settings are under System › Windows settings.</Note>
      </div>
    </div>
  );
}
