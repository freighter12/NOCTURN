// Smart playlists: the hook that keeps them up to date, their cover art,
// and the rule editor.

import { useCallback, useEffect, useMemo, useState } from "react";
import { I } from "../components/Icons";
import { errorMessage, music, type MusicStats, type SmartPlaylistRecord } from "../lib/api";
import { useApp } from "../lib/app";
import { useMusic } from "./MusicProvider";
import {
  builtins, customLists, DATE_FIELDS, defaultRule, describe, evaluate, FIELD_LABEL, NUM_FIELDS, sanitizeRules, SORT_LABEL, TEXT_FIELDS,
  type Field, type Rule, type SmartList, type SmartRules, type SmartSort, DEFAULT_RULES,
} from "./smart";

/** Built-in mixes and the user's own smart lists, worked out from the library. */
export function useSmartLists() {
  const m = useMusic();
  const [stats, setStats] = useState<MusicStats | null>(null);
  const [records, setRecords] = useState<SmartPlaylistRecord[]>([]);
  const reloadCustom = useCallback(() => music.smartList().then(setRecords).catch(() => {}), []);
  // Fresh statistics when the library or the playing song changes.
  useEffect(() => {
    const t = window.setTimeout(() => music.stats().then(setStats).catch(() => {}), 300);
    return () => window.clearTimeout(t);
  }, [m.tracks.length, m.current?.id]);
  useEffect(() => {
    reloadCustom();
  }, [reloadCustom]);
  const hour = new Date().getHours();
  const built = useMemo(() => builtins(m.tracks, stats), [m.tracks, stats, hour]); // eslint-disable-line react-hooks/exhaustive-deps
  const custom = useMemo(() => customLists(records, m.tracks), [records, m.tracks]);
  const find = useCallback((id: string) => [...built, ...custom].find((l) => l.id === id) ?? null, [built, custom]);
  return { builtins: built, custom, find, reloadCustom, loaded: stats != null };
}

/** The songs of a smart list or playlist by ID, worked out right now (for wind-down). */
export async function resolveList(id: string, tracks: Parameters<typeof builtins>[0], playlists: { id: string; tracks: number[] }[]): Promise<number[]> {
  if (id.startsWith("builtin:")) {
    const stats = await music.stats().catch(() => null);
    return builtins(tracks, stats).find((l) => l.id === id)?.ids ?? [];
  }
  if (id.startsWith("smart-")) {
    const recs = await music.smartList().catch(() => []);
    const r = recs.find((x) => x.id === id);
    return r ? evaluate(sanitizeRules(r.rules), tracks, Date.now(), r.id) : [];
  }
  return playlists.find((p) => p.id === id)?.tracks ?? [];
}

export function SmartArt({ list, size }: { list: Pick<SmartList, "glyph" | "colour">; size: number }) {
  return (
    <span className="art art-empty smart-art" style={{ width: size, height: size, background: `linear-gradient(140deg, ${list.colour}, #0d1120)`, fontSize: size * 0.34 }} aria-hidden>
      {list.glyph}
    </span>
  );
}

// ------------------------------------------------------------------ editor

const TEXT_OPS = [["contains", "contains"], ["not_contains", "doesn't contain"], ["is", "is"], ["starts", "starts with"]] as const;
const NUM_OPS = [["gt", "is over"], ["lt", "is under"], ["eq", "is"]] as const;
const DATE_OPS = [["within", "in the last"], ["older", "more than"]] as const;

function RuleRow({ rule, onChange, onRemove }: { rule: Rule; onChange: (r: Rule) => void; onRemove: () => void }) {
  const fields = Object.keys(FIELD_LABEL) as Field[];
  return (
    <div className="smart-rule">
      <select className="select" aria-label="Field" value={rule.field} onChange={(e) => onChange(defaultRule(e.target.value as Field))}>
        {fields.map((f) => <option key={f} value={f}>{FIELD_LABEL[f]}</option>)}
      </select>
      {(TEXT_FIELDS as string[]).includes(rule.field) && typeof rule.value === "string" && (
        <>
          <select className="select" aria-label="Condition" value={rule.op} onChange={(e) => onChange({ ...rule, op: e.target.value } as Rule)}>
            {TEXT_OPS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
          <input className="input" aria-label="Value" maxLength={100} value={rule.value} placeholder="e.g. jazz" onChange={(e) => onChange({ ...rule, value: e.target.value } as Rule)} />
        </>
      )}
      {(NUM_FIELDS as string[]).includes(rule.field) && typeof rule.value === "number" && (
        <>
          <select className="select" aria-label="Condition" value={rule.op} onChange={(e) => onChange({ ...rule, op: e.target.value } as Rule)}>
            {NUM_OPS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
          <input className="input" type="number" aria-label="Value" value={rule.value} onChange={(e) => onChange({ ...rule, value: Number(e.target.value) || 0 } as Rule)} />
        </>
      )}
      {(DATE_FIELDS as string[]).includes(rule.field) && typeof rule.value === "number" && (
        <>
          <select className="select" aria-label="Condition" value={rule.op} onChange={(e) => onChange({ ...rule, op: e.target.value } as Rule)}>
            {DATE_OPS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            {rule.field === "played" && <option value="never">never</option>}
          </select>
          {rule.op !== "never" && (
            <span className="row" style={{ gap: 6 }}>
              <input className="input" type="number" min={0} max={3650} aria-label="Days" style={{ width: 80 }} value={rule.value} onChange={(e) => onChange({ ...rule, value: Math.max(0, Number(e.target.value) || 0) } as Rule)} />
              <span className="small dim">days{rule.op === "older" ? " ago" : ""}</span>
            </span>
          )}
        </>
      )}
      {rule.field === "liked" && (
        <select className="select" aria-label="Condition" value={rule.op} onChange={(e) => onChange({ field: "liked", op: e.target.value === "not" ? "not" : "is", value: true })}>
          <option value="is">yes</option>
          <option value="not">no</option>
        </select>
      )}
      {rule.field === "source" && (
        <select className="select" aria-label="Source" value={rule.value} onChange={(e) => onChange({ field: "source", op: "is", value: e.target.value === "youtube" ? "youtube" : "local" })}>
          <option value="local">your own files</option>
          <option value="youtube">streamed from YouTube</option>
        </select>
      )}
      <button className="btn ghost icon sm" aria-label="Remove rule" onClick={onRemove}><I.close /></button>
    </div>
  );
}

export function SmartEditor({ existing, onDone, onCancel }: { existing: SmartList | null; onDone: (id: string) => void; onCancel: () => void }) {
  const m = useMusic();
  const { toast } = useApp();
  const [name, setName] = useState(existing?.name ?? "");
  const [rules, setRules] = useState<SmartRules>(existing?.rules ?? DEFAULT_RULES);
  const [busy, setBusy] = useState(false);
  const preview = useMemo(() => evaluate(rules, m.tracks, Date.now(), existing?.id ?? "new"), [rules, m.tracks, existing?.id]);
  const set = (patch: Partial<SmartRules>) => setRules((r) => ({ ...r, ...patch }));
  async function save() {
    setBusy(true);
    try {
      const saved = await music.smartSave({ id: existing?.id ?? null, name: name.trim() || "Smart playlist", rules });
      toast({ tone: "good", title: existing ? "Smart playlist saved" : `"${saved.name}" created`, detail: "It keeps itself up to date as your library and listening change." });
      onDone(saved.id);
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save", detail: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="stack smart-editor" style={{ gap: 16 }}>
      <button className="btn ghost sm" style={{ alignSelf: "flex-start", justifySelf: "start" }} onClick={onCancel}>← Back</button>
      <div>
        <div className="eyebrow small dim">{existing ? "Edit smart playlist" : "New smart playlist"}</div>
        <input className="input mt-1" style={{ fontSize: 18, height: 42, maxWidth: 420 }} maxLength={80} placeholder="Name, e.g. Jazz I love" value={name} onChange={(e) => setName(e.target.value)} aria-label="Smart playlist name" autoFocus />
      </div>
      <div className="panel" style={{ padding: 16 }}>
        <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
          <span>Songs that match</span>
          <select className="select" aria-label="Match" value={rules.match} onChange={(e) => set({ match: e.target.value === "any" ? "any" : "all" })}>
            <option value="all">all</option>
            <option value="any">any</option>
          </select>
          <span>of these:</span>
        </div>
        <div className="stack mt-2" style={{ gap: 8 }}>
          {rules.rules.map((r, i) => (
            <RuleRow key={i} rule={r} onChange={(nr) => set({ rules: rules.rules.map((x, k) => (k === i ? nr : x)) })} onRemove={() => set({ rules: rules.rules.filter((_, k) => k !== i) })} />
          ))}
          {!rules.rules.length && <div className="small dim">No rules: every song in your library.</div>}
          <button className="btn ghost sm" style={{ alignSelf: "flex-start", justifySelf: "start" }} disabled={rules.rules.length >= 20} onClick={() => set({ rules: [...rules.rules, defaultRule("artist")] })}><I.plus /> Add a rule</button>
        </div>
        <div className="row mt-2" style={{ gap: 8, flexWrap: "wrap" }}>
          <span>Order by</span>
          <select className="select" aria-label="Order" value={rules.sort} onChange={(e) => set({ sort: e.target.value as SmartSort })}>
            {(Object.keys(SORT_LABEL) as SmartSort[]).map((s) => <option key={s} value={s}>{SORT_LABEL[s]}</option>)}
          </select>
          <span>and keep</span>
          <select className="select" aria-label="Limit" value={rules.limit ?? 0} onChange={(e) => set({ limit: Number(e.target.value) || null })}>
            {[25, 50, 100, 250, 500].map((n) => <option key={n} value={n}>up to {n} songs</option>)}
            <option value={0}>every match</option>
          </select>
        </div>
      </div>
      <div className="small dim">{describe(rules)}</div>
      <div className="row" style={{ gap: 8 }}>
        <button className="btn primary" disabled={busy} onClick={save}>{existing ? "Save" : "Create smart playlist"}</button>
        <button className="btn ghost" onClick={onCancel}>Cancel</button>
        <span className="small dim">{preview.length ? `${preview.length} song${preview.length === 1 ? "" : "s"} right now` : "No songs match yet"}</span>
      </div>
      {preview.length > 0 && (
        <div className="list">
          {preview.slice(0, 12).map((id) => {
            const t = m.byId.get(id);
            return t ? <div key={id} className="list-item small"><div className="body"><div className="title">{t.title}</div><div className="desc">{t.artist}{t.genre ? ` · ${t.genre}` : ""} · {t.plays} plays</div></div></div> : null;
          })}
          {preview.length > 12 && <div className="small dim" style={{ padding: "6px 12px" }}>…and {preview.length - 12} more</div>}
        </div>
      )}
    </div>
  );
}
