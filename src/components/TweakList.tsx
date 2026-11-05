// Selectable list of catalogue optimisations with live status.

import { useMemo, useState } from "react";
import type { Category, TweakView } from "../lib/api";
import { useApp } from "../lib/app";
import { takesLabel } from "../lib/format";
import { I } from "./Icons";
import { Empty, RiskTag } from "./ui";

const stateText: Record<string, string> = {
  applied: "Applied",
  not_applied: "Not applied",
  partial: "Partly applied",
  action: "One-time action",
  unavailable: "Unavailable",
  unknown: "Couldn't read",
};

export function TweakList({ tweaks, category, source = "user" }: { tweaks: TweakView[]; category?: Category | Category[]; source?: string }) {
  const { requestApply } = useApp();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<string | null>(null);
  const list = useMemo(() => {
    const cats = category ? (Array.isArray(category) ? category : [category]) : null;
    return tweaks.filter((t) => !cats || cats.includes(t.category));
  }, [tweaks, category]);

  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });

  if (!list.length) return <Empty title="Nothing here yet" />;
  const applicable = list.filter((t) => t.status.state !== "applied" && t.status.state !== "unavailable");

  return (
    <div>
      <div className="row between" style={{ padding: "10px 16px", borderBottom: "1px solid var(--line)" }}>
        <label className="row small muted" style={{ gap: 8 }}>
          <input
            type="checkbox"
            className="check"
            checked={applicable.length > 0 && applicable.every((t) => selected.has(t.id))}
            onChange={(e) => setSelected(e.target.checked ? new Set(applicable.map((t) => t.id)) : new Set())}
            aria-label="Select all not yet applied"
          />
          Select all not yet applied
        </label>
        <div className="row">
          {selected.size > 0 && (
            <button className="btn ghost sm" onClick={() => setSelected(new Set())}>
              Clear
            </button>
          )}
          <button
            className="btn primary sm"
            disabled={!selected.size}
            onClick={() =>
              requestApply({
                tweakIds: [...selected],
                source,
                label: selected.size === 1 ? list.find((t) => selected.has(t.id))!.title : `Custom: ${selected.size} optimisations`,
                onDone: (o) => o.approved && setSelected(new Set()),
              })
            }
          >
            Review {selected.size || ""} selected
          </button>
        </div>
      </div>
      {list.map((t) => (
        <div className="tweak" key={t.id}>
          <input type="checkbox" className="check" style={{ marginTop: 3 }} checked={selected.has(t.id)} disabled={t.status.state === "unavailable"} onChange={() => toggle(t.id)} aria-label={`Select ${t.title}`} />
          <div style={{ minWidth: 0 }}>
            <div className="title">
              {t.title}
              <RiskTag risk={t.risk} />
              {t.requires_admin && (
                <span className="tag" title="Needs administrator approval">
                  <I.lock /> Admin
                </span>
              )}
              {!t.reversible && <span className="tag warning">One-time</span>}
            </div>
            <div className="summary">{t.summary}</div>
            {open === t.id && (
              <>
                <div className="details">{t.details}</div>
                <div className="details">{takesLabel[t.takes]}.</div>
              </>
            )}
            {t.caveat && <div className="caveat">{t.caveat}</div>}
            <button className="btn ghost sm" style={{ marginTop: 6, marginLeft: -10 }} onClick={() => setOpen(open === t.id ? null : t.id)}>
              {open === t.id ? "Less" : "What exactly changes?"}
            </button>
          </div>
          <div className="state">
            <span className="row small" style={{ gap: 6, color: t.status.state === "applied" ? "var(--ink-0)" : "var(--ink-2)" }}>
              {t.status.state === "applied" && <I.check style={{ width: 13, height: 13, color: "var(--good)" }} />}
              {stateText[t.status.state]}
            </span>
            {t.status.detail && <span className="small dim">{t.status.detail}</span>}
            <button
              className="btn sm"
              disabled={t.status.state === "applied" || t.status.state === "unavailable"}
              onClick={() => requestApply({ tweakIds: [t.id], source, label: t.title, title: t.title })}
            >
              {t.one_shot ? "Run" : "Apply"}
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
