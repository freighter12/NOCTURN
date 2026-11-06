// Add, remove, reorder and resize dashboard widgets. Used for the main Home
// dashboard (Experience Studio) and for each workspace's own dashboard.

import { useRef } from "react";
import { I } from "../components/Icons";
import { Panel, Seg } from "../components/ui";
import type { WidgetSize, WidgetSlot } from "./prefs";
import { WIDGETS, widgetDef } from "./widgets";

export function move<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length || from === to) return list;
  const copy = [...list];
  const [x] = copy.splice(from, 1);
  copy.splice(to, 0, x);
  return copy;
}

export function DashboardEditor({ widgets, onChange, title = "On your dashboard", actions }: { widgets: WidgetSlot[]; onChange: (w: WidgetSlot[]) => void; title?: string; actions?: React.ReactNode }) {
  const drag = useRef<number | null>(null);
  const set = (fn: (w: WidgetSlot[]) => WidgetSlot[]) => onChange(fn(widgets));
  return (
    <div className="stack" style={{ gap: 14 }}>
      <Panel title={title} sub="Drag to reorder. Small is a quarter of the row, medium a half, large the full width." actions={actions}>
        {widgets.length === 0 && <div className="small dim">No widgets. Add some below.</div>}
        <div className="list">
          {widgets.map((w, i) => (
            <div
              key={w.id}
              className="list-item draggable"
              draggable
              onDragStart={() => (drag.current = i)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => {
                if (drag.current != null) set((x) => move(x, drag.current!, i));
                drag.current = null;
              }}
              style={{ alignItems: "center" }}
            >
              <span className="drag-handle" aria-hidden>⋮⋮</span>
              <div className="body">
                <div className="title">{widgetDef(w.type).name}</div>
                <div className="desc">{widgetDef(w.type).description}</div>
              </div>
              <Seg<WidgetSize> value={w.size} onChange={(v) => set((x) => x.map((y) => (y.id === w.id ? { ...y, size: v } : y)))} options={[{ value: "s", label: "S" }, { value: "m", label: "M" }, { value: "l", label: "L" }]} />
              <button className="btn ghost sm icon" aria-label="Move up" disabled={i === 0} onClick={() => set((x) => move(x, i, i - 1))}>↑</button>
              <button className="btn ghost sm icon" aria-label="Move down" disabled={i === widgets.length - 1} onClick={() => set((x) => move(x, i, i + 1))}>↓</button>
              <button className="btn ghost sm icon" aria-label={`Remove ${widgetDef(w.type).name}`} onClick={() => set((x) => x.filter((y) => y.id !== w.id))}><I.close /></button>
            </div>
          ))}
        </div>
      </Panel>
      <Panel title="Add a widget">
        <div className="grid cols-3" style={{ gap: 10 }}>
          {WIDGETS.map((d) => (
            <button key={d.type} className="add-widget" disabled={widgets.length >= 24} onClick={() => set((x) => [...x, { id: `w-${d.type}-${Date.now().toString(36)}`, type: d.type, size: d.type === "vitals" ? "l" : "m" }])}>
              <b>{d.name}</b>
              <span className="small muted">{d.description}</span>
            </button>
          ))}
        </div>
      </Panel>
    </div>
  );
}
