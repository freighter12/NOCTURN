// Home: the personal dashboard. Which widgets appear, their order and size
// come from preferences. "Arrange" lets you drag widgets around and resize
// them right here; Customise › Home page has the full editor.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { I } from "../components/Icons";
import { SkyHeader } from "../shell/SkyHeader";
import { useApp } from "../lib/app";
import { usePrefs } from "../shell/PrefsProvider";
import { widgetDef } from "../shell/widgets";
import { greetingSubtitle, greetingTitle } from "../shell/greeting";
import { move } from "../shell/DashboardEditor";
import type { WidgetSize, WidgetSlot } from "../shell/prefs";

const SIZES: { value: WidgetSize; label: string; name: string }[] = [
  { value: "s", label: "S", name: "Small" },
  { value: "m", label: "M", name: "Medium" },
  { value: "l", label: "L", name: "Large" },
];

export function Home() {
  const { prefs, update } = usePrefs();
  const { navigate } = useApp();
  const ws = prefs.workspaces.list.find((w) => w.id === prefs.workspaces.active);
  // A workspace with its own dashboard replaces the main one while active.
  const saved = ws?.widgets ?? prefs.home.widgets;
  const g = prefs.home.greeting;
  const [arranging, setArranging] = useState(false);

  const save = (next: WidgetSlot[]) =>
    update((p) =>
      ws?.widgets
        ? { ...p, workspaces: { ...p.workspaces, list: p.workspaces.list.map((w) => (w.id === ws.id ? { ...w, widgets: next } : w)) } }
        : { ...p, home: { ...p.home, widgets: next } },
    );

  useEffect(() => {
    if (!arranging) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setArranging(false);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [arranging]);

  return (
    <div className="page">
      <SkyHeader
        eyebrow={ws ? `Workspace · ${ws.name}` : "Nocturn"}
        title={greetingTitle(g)}
        sky={g.sky}
        showMoon={g.showMoon}
        showClock={g.showClock}
        showDate={g.showDate}
        actions={
          <div className="row" style={{ gap: 8 }}>
            {arranging ? (
              <button className="btn primary" onClick={() => setArranging(false)}>
                <I.check /> Done
              </button>
            ) : (
              <button className="btn ghost sky-btn" onClick={() => setArranging(true)} disabled={saved.length === 0}>
                <I.grid /> Arrange
              </button>
            )}
            <button className="btn ghost sky-btn" onClick={() => navigate("studio")}>
              <I.studio /> Customise
            </button>
          </div>
        }
      >
        {greetingSubtitle(g)}
      </SkyHeader>
      {saved.length === 0 ? (
        <div className="panel">
          <div className="small muted">Your dashboard is empty. Add widgets in Customise › Home page.</div>
        </div>
      ) : (
        <Widgets widgets={saved} arranging={arranging} onChange={save} />
      )}
    </div>
  );
}

/**
 * The widget grid. While arranging, each widget is covered by a handle you
 * can drag (the others slide out of the way as you go) with size and remove
 * buttons; the new order is saved when you let go.
 */
function Widgets({ widgets, arranging, onChange }: { widgets: WidgetSlot[]; arranging: boolean; onChange: (w: WidgetSlot[]) => void }) {
  const grid = useRef<HTMLDivElement>(null);
  // While dragging, the live order (saved on drop); otherwise the saved one.
  const [live, setLive] = useState<WidgetSlot[] | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const list = live ?? widgets;
  const rects = useRef(new Map<string, DOMRect>());
  const pointer = useRef<{ id: string; dx: number; dy: number; x: number; y: number } | null>(null);
  const [ghost, setGhost] = useState<{ x: number; y: number } | null>(null);

  // FLIP: when the order changes, slide each widget from where it was.
  const snapshot = () => {
    const m = new Map<string, DOMRect>();
    grid.current?.querySelectorAll<HTMLElement>("[data-wid]").forEach((el) => m.set(el.dataset.wid!, el.getBoundingClientRect()));
    rects.current = m;
  };
  useLayoutEffect(() => {
    const before = rects.current;
    if (!before.size || !grid.current) return;
    rects.current = new Map();
    if (document.body.classList.contains("reduce-motion")) return;
    grid.current.querySelectorAll<HTMLElement>("[data-wid]").forEach((el) => {
      const a = before.get(el.dataset.wid!);
      if (!a || el.dataset.wid === dragId) return;
      const b = el.getBoundingClientRect();
      const dx = a.left - b.left;
      const dy = a.top - b.top;
      if (!dx && !dy) return;
      el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: 220, easing: "cubic-bezier(0.2, 0.7, 0.2, 1)" });
    });
  }, [list, dragId]);

  const change = (next: WidgetSlot[]) => {
    snapshot();
    onChange(next);
  };

  function onPointerDown(e: React.PointerEvent, id: string) {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    const slot = (e.currentTarget as HTMLElement).closest<HTMLElement>("[data-wid]")!;
    const r = slot.getBoundingClientRect();
    pointer.current = { id, dx: e.clientX - r.left, dy: e.clientY - r.top, x: e.clientX, y: e.clientY };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: React.PointerEvent) {
    const p = pointer.current;
    if (!p) return;
    // A few pixels of slack so a click doesn't start a drag.
    if (!dragId && Math.hypot(e.clientX - p.x, e.clientY - p.y) < 5) return;
    if (!dragId) {
      setDragId(p.id);
      setLive(widgets);
    }
    setGhost({ x: e.clientX - p.dx, y: e.clientY - p.dy });
    const cur = live ?? widgets;
    const over = document.elementsFromPoint(e.clientX, e.clientY).map((el) => (el as HTMLElement).closest<HTMLElement>("[data-wid]")).find((el) => el && el.dataset.wid !== p.id);
    if (!over) return;
    const from = cur.findIndex((w) => w.id === p.id);
    const to = cur.findIndex((w) => w.id === over.dataset.wid);
    if (from < 0 || to < 0 || from === to) return;
    snapshot();
    setLive(move(cur, from, to));
  }

  function onPointerUp() {
    const p = pointer.current;
    pointer.current = null;
    if (!p || !dragId) return;
    const final = live;
    setDragId(null);
    setGhost(null);
    setLive(null);
    if (final && final.map((w) => w.id).join() !== widgets.map((w) => w.id).join()) onChange(final);
  }

  const dragged = dragId ? list.find((w) => w.id === dragId) : null;

  return (
    <>
      <div ref={grid} className={`widgets ${arranging ? "arranging" : ""}`}>
        {list.map((w, i) => {
          const def = widgetDef(w.type);
          return (
            <div key={w.id} data-wid={w.id} className={`widget-slot size-${w.size} ${dragId === w.id ? "dragging" : ""}`}>
              {def.render()}
              {arranging && (
                <div
                  className="arrange-cover"
                  onPointerDown={(e) => onPointerDown(e, w.id)}
                  onPointerMove={onPointerMove}
                  onPointerUp={onPointerUp}
                  onPointerCancel={onPointerUp}
                >
                  <div className="arrange-bar">
                    <span className="arrange-name"><span className="drag-handle" aria-hidden>⋮⋮</span>{def.name}</span>
                    <span className="row" style={{ gap: 4 }}>
                      <span className="arrange-sizes" role="radiogroup" aria-label={`${def.name} size`}>
                        {SIZES.map((s) => (
                          <button key={s.value} role="radio" aria-checked={w.size === s.value} aria-label={s.name} title={s.name} className={w.size === s.value ? "on" : ""} onClick={() => change(list.map((x) => (x.id === w.id ? { ...x, size: s.value } : x)))}>
                            {s.label}
                          </button>
                        ))}
                      </span>
                      <button className="btn ghost sm icon" aria-label={`Move ${def.name} earlier`} title="Move earlier" disabled={i === 0} onClick={() => change(move(list, i, i - 1))}>←</button>
                      <button className="btn ghost sm icon" aria-label={`Move ${def.name} later`} title="Move later" disabled={i === list.length - 1} onClick={() => change(move(list, i, i + 1))}>→</button>
                      <button className="btn ghost sm icon" aria-label={`Remove ${def.name}`} title="Remove" onClick={() => change(list.filter((x) => x.id !== w.id))}><I.close /></button>
                    </span>
                  </div>
                  <div className="arrange-hint">Drag to move</div>
                </div>
              )}
            </div>
          );
        })}
      </div>
      {dragged && ghost && (
        <div className="arrange-ghost" style={{ left: ghost.x, top: ghost.y, width: grid.current?.querySelector<HTMLElement>(`[data-wid="${dragged.id}"]`)?.offsetWidth }}>
          <span className="drag-handle" aria-hidden>⋮⋮</span> {widgetDef(dragged.type).name}
        </div>
      )}
    </>
  );
}
