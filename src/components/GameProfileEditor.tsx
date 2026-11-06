// Per-game profile: priority and CPUs at launch (saved as a process rule),
// GPU preference and fullscreen optimisations (journaled registry changes).
// Edit → preview exactly what changes → apply → verified result with Undo.

import { useEffect, useMemo, useState } from "react";
import { api, errorMessage, type GameProfile, type GameView, type GpuPreference, type Priority, type ProfilePreviewItem, type Progress } from "../lib/api";
import { useApp } from "../lib/app";
import { maskBits, PRIORITIES, toMask } from "../lib/cpu";
import { useTelemetry } from "../lib/telemetry";
import { I } from "./Icons";
import { ErrorNote, Note, Panel, Spinner, Toggle } from "./ui";

type Phase = "edit" | "previewing" | "review" | "running";

const GPU_OPTIONS: { value: GpuPreference | "unchanged"; label: string }[] = [
  { value: "unchanged", label: "Leave unchanged" },
  { value: "default", label: "Let Windows decide" },
  { value: "power_saving", label: "Power saving GPU" },
  { value: "high_performance", label: "High performance GPU" },
];

function fileName(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

export function GameProfileEditor({ game, onClose, onSaved }: { game: GameView; onClose: () => void; onSaved: () => void }) {
  const { toast, bumpData } = useApp();
  const { latest } = useTelemetry();
  const logical = latest?.cpu_per_core.length ?? 0;
  const systemBits = useMemo(() => Array.from({ length: logical }, (_, i) => i), [logical]);

  const saved = game.profile;
  const [exe, setExe] = useState(saved?.exe ?? game.exes[0] ?? "");
  const [priority, setPriority] = useState<Priority>(saved?.priority ?? "normal");
  const [cpus, setCpus] = useState<Set<number>>(new Set());
  const [gpu, setGpu] = useState<GpuPreference | "unchanged">(saved?.gpu_preference ?? "unchanged");
  const [fso, setFso] = useState(saved?.disable_fso ?? false);
  const [phase, setPhase] = useState<Phase>("edit");
  const [items, setItems] = useState<ProfilePreviewItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);

  // Reset when switching games.
  useEffect(() => {
    setExe(saved?.exe ?? game.exes[0] ?? "");
    setPriority(saved?.priority ?? "normal");
    setGpu(saved?.gpu_preference ?? "unchanged");
    setFso(saved?.disable_fso ?? false);
    setPhase("edit");
    setError(null);
    setItems([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.id]);

  // CPU selection: saved mask, else all CPUs (once the CPU count is known).
  useEffect(() => {
    if (!logical) return;
    setCpus(saved?.affinity ? maskBits(saved.affinity) : new Set(systemBits));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.id, logical]);

  const allCpus = systemBits.length > 0 && systemBits.every((b) => cpus.has(b));
  const profile: GameProfile = {
    game_id: game.id,
    exe,
    priority: priority === "normal" ? null : priority,
    affinity: allCpus || cpus.size === 0 ? null : toMask(cpus),
    gpu_preference: gpu === "unchanged" ? null : gpu,
    disable_fso: fso,
    updated_at: "",
  };
  // An empty profile is still worth previewing when one is saved: it removes the launch rule.
  const empty = !saved && !profile.priority && !profile.affinity && !profile.gpu_preference && !profile.disable_fso;
  const changes = items.filter((i) => i.will_change).length;

  // Any edit invalidates a shown preview.
  const edit = <T,>(set: (v: T) => void) => (v: T) => {
    set(v);
    if (phase === "review") setPhase("edit");
  };

  async function preview() {
    setPhase("previewing");
    setError(null);
    try {
      setItems(await api.previewGameProfile(profile));
      setPhase("review");
    } catch (e) {
      setError(errorMessage(e));
      setPhase("edit");
    }
  }

  async function apply() {
    setPhase("running");
    setError(null);
    try {
      const out = await api.applyGameProfile(profile, game.name, setProgress);
      const failed = out.report?.results.filter((r) => r.status !== "applied") ?? [];
      const problems = [...failed.map((f) => `${f.target}: ${f.error ?? f.status}`), ...(out.rule_error ? [`Launch rule: ${out.rule_error}`] : [])];
      const tx = out.report?.tx_id;
      toast({
        tone: problems.length ? "warning" : "good",
        title: problems.length ? `Profile for ${game.name} partly applied` : `Profile saved for ${game.name}`,
        detail: problems.length
          ? problems.join(" · ")
          : game.running
            ? "Restart the game for GPU and fullscreen settings to take effect."
            : "Takes effect the next time the game starts.",
        action: tx ? { label: "Undo", run: () => api.rollback(tx).then(() => { bumpData(); onSaved(); }) } : undefined,
      });
      bumpData();
      onSaved();
      setPhase("edit");
      setItems([]);
    } catch (e) {
      setError(errorMessage(e));
      setPhase("review");
    } finally {
      setProgress(null);
    }
  }

  const busy = phase === "previewing" || phase === "running";

  return (
    <Panel
      lit
      title={game.name}
      sub={`${game.launcher}${saved ? ` · profile saved` : ""}${game.running ? " · running now" : ""}`}
      actions={<button className="btn ghost sm icon" onClick={onClose} aria-label="Close profile"><I.x /></button>}
    >
      {game.exes.length === 0 ? (
        <Note tone="warning">NOCTURN couldn't find this game's executable in its install folder. Add it with "Add a game" using the full path to its .exe.</Note>
      ) : (
        <div className="stack" style={{ gap: 16 }}>
          <div className="field">
            <label htmlFor="gexe">Game executable</label>
            <select id="gexe" className="select" value={exe} disabled={busy} onChange={(e) => edit(setExe)(e.target.value)}>
              {game.exes.map((x) => <option key={x} value={x}>{fileName(x)}</option>)}
            </select>
            <div className="hint mono truncate" title={exe}>{exe}</div>
          </div>

          <div className="field">
            <label htmlFor="gprio">Priority at launch</label>
            <select id="gprio" className="select" value={priority} disabled={busy} onChange={(e) => edit(setPriority)(e.target.value as Priority)}>
              {PRIORITIES.map((p) => <option key={p.value} value={p.value}>{p.label}{p.value === "normal" ? " (no change)" : ""}</option>)}
            </select>
            <div className="hint">{PRIORITIES.find((p) => p.value === priority)?.hint}</div>
          </div>

          <div className="field">
            <label>CPUs the game may use</label>
            {logical === 0 ? (
              <div className="hint">Waiting for telemetry…</div>
            ) : (
              <>
                <div className="cpu-grid" role="group" aria-label="CPU affinity">
                  {systemBits.map((b) => (
                    <label key={b} className={`cpu-cell ${cpus.has(b) ? "on" : ""}`} title={`Logical CPU ${b}`}>
                      <input
                        type="checkbox"
                        checked={cpus.has(b)}
                        disabled={busy}
                        onChange={() => {
                          const n = new Set(cpus);
                          n.has(b) ? n.delete(b) : n.add(b);
                          edit(setCpus)(n);
                        }}
                      />
                      {b}
                    </label>
                  ))}
                </div>
                <div className="row">
                  <button className="btn ghost sm" disabled={busy} onClick={() => edit(setCpus)(new Set(systemBits))}>All CPUs</button>
                  <button className="btn ghost sm" disabled={busy} onClick={() => edit(setCpus)(new Set(systemBits.filter((b) => b !== 0)))}>All except CPU 0</button>
                </div>
              </>
            )}
            <div className="hint">Leave on all CPUs unless you've measured a benefit — restricting cores usually lowers FPS in modern games.</div>
          </div>

          <div className="field">
            <label htmlFor="ggpu">Graphics preference</label>
            <select id="ggpu" className="select" value={gpu} disabled={busy} onChange={(e) => edit(setGpu)(e.target.value as GpuPreference | "unchanged")}>
              {GPU_OPTIONS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
            </select>
            <div className="hint">The same setting as Windows Settings › Display › Graphics. Only matters on PCs with two GPUs, such as most gaming laptops.</div>
          </div>

          <div className="list-item" style={{ alignItems: "center", border: 0, padding: 0 }}>
            <div className="body">
              <div className="title">Disable fullscreen optimisations</div>
              <div className="desc">The exe's Properties › Compatibility checkbox. Try it only if this game stutters in fullscreen; it can turn off Auto HDR for the game. Other compatibility flags are kept.</div>
            </div>
            <Toggle on={fso} disabled={busy} onChange={edit(setFso)} label="Disable fullscreen optimisations" />
          </div>
          {saved?.disable_fso && !fso && (
            <div className="small dim">Turning this off here doesn't remove the flag — undo the original change from History.</div>
          )}

          {error && <ErrorNote message={error} />}

          {phase === "review" && (
            <div className="stack" style={{ gap: 8 }}>
              <div className="section-title" style={{ margin: 0 }}>What will change</div>
              {items.map((i) => (
                <div key={i.target} className="stack" style={{ gap: 3, padding: "10px 0", borderTop: "1px solid var(--line)", opacity: i.will_change ? 1 : 0.55 }}>
                  <div className="row between">
                    <span className="title">{i.title}</span>
                    {!i.will_change && <span className="tag solid">Already set</span>}
                  </div>
                  <div className="small mono dim truncate" title={i.target}>{i.target}</div>
                  <div className="small"><span className="dim">Now:</span> {i.current} <span className="dim">→</span> {i.proposed}</div>
                  {i.note && <div className="small dim">{i.note}</div>}
                </div>
              ))}
              <p className="small dim" style={{ margin: 0 }}>
                Registry values are recorded first and can be rolled back from History. The launch rule can be edited or removed on the Processes page.
              </p>
            </div>
          )}

          <div className="row between">
            <span className="small dim">{phase === "running" && progress ? progress.message : empty ? "Choose at least one setting." : ""}</span>
            {phase === "review" ? (
              <span className="row" style={{ gap: 6 }}>
                <button className="btn ghost" onClick={() => setPhase("edit")}>Back</button>
                <button className="btn primary" disabled={changes === 0} onClick={apply}>
                  {changes === 0 ? "Nothing to change" : `Apply ${changes} change${changes === 1 ? "" : "s"}`}
                </button>
              </span>
            ) : (
              <button className="btn primary" disabled={busy || empty || !exe} onClick={preview}>
                {busy ? <Spinner /> : null} Preview changes <I.arrow />
              </button>
            )}
          </div>
        </div>
      )}
    </Panel>
  );
}
