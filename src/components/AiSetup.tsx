// "Set up AI for me": one click to a working assistant.
// Free & private: installs Ollama if needed (official, checksum-verified),
// downloads a model sized for this PC and connects NOCTURN to it.
// Claude: opens the key page, paste, done.

import { useEffect, useState } from "react";
import { I } from "./Icons";
import { ErrorNote, Note, Spinner } from "./ui";
import { api, errorMessage, type AiSetupDetect, type AiSetupProgress, type AppSettings } from "../lib/api";
import { useApp } from "../lib/app";

type Phase = "choose" | "local" | "claude" | "working" | "done";

export function AiSetup({ compact, onDone }: { compact?: boolean; onDone?: (s: AppSettings) => void }) {
  const { settings, setSettings, toast } = useApp();
  const [det, setDet] = useState<AiSetupDetect | null>(null);
  const [detErr, setDetErr] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>("choose");
  const [model, setModel] = useState<string>("");
  const [prog, setProg] = useState<AiSetupProgress | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [key, setKey] = useState("");
  const [open, setOpen] = useState(!compact);

  const detect = () => {
    setDetErr(null);
    api
      .aiSetupDetect()
      .then((d) => {
        setDet(d);
        setModel((m) => m || (d.models.includes(d.recommended.id) ? d.recommended.id : d.recommended.id));
      })
      .catch((e) => setDetErr(errorMessage(e)));
  };
  useEffect(() => {
    if (open) detect();
  }, [open]);

  async function finish() {
    const s = await api.getSettings();
    setSettings(s);
    onDone?.(s);
    try {
      const t = await api.aiTest();
      setResult({ ok: t.ok, message: t.ok ? "NOCTURN AI is ready. Ask it anything about your PC." : t.message });
    } catch (e) {
      setResult({ ok: false, message: errorMessage(e) });
    }
    setPhase("done");
  }

  async function runLocal() {
    setErr(null);
    setResult(null);
    setProg({ stage: "start", label: "Getting ready", fraction: null, detail: null });
    setPhase("working");
    try {
      await api.aiSetupLocal(model, setProg);
      await finish();
    } catch (e) {
      const m = errorMessage(e);
      setErr(m.includes("cancelled") ? null : m);
      setPhase("local");
    }
  }

  async function runClaude() {
    if (!settings || !key.trim()) return;
    setErr(null);
    setResult(null);
    setPhase("working");
    setProg({ stage: "configure", label: "Saving your key and checking it", fraction: null, detail: null });
    try {
      await api.aiSetKey("anthropic", key.trim());
      const ai = { ...settings.ai, provider: "anthropic" as const, model: settings.ai.provider === "anthropic" && settings.ai.model.startsWith("claude-") ? settings.ai.model : "claude-opus-5-5" };
      await api.saveSettings({ ...settings, ai });
      setKey("");
      await finish();
    } catch (e) {
      setErr(errorMessage(e));
      setPhase("claude");
    }
  }

  if (!open) {
    return (
      <div className="ai-setup compact">
        <span className="ai-setup-orb" aria-hidden><I.assistant /></span>
        <div className="grow">
          <b>Set up AI for me</b>
          <div className="small dim">Pick free &amp; private (runs on this PC) or Claude, and NOCTURN does the rest.</div>
        </div>
        <button className="btn sm" onClick={() => setOpen(true)}>Start</button>
      </div>
    );
  }

  const already = det?.models.includes(model);
  const choice = det?.options.find((o) => o.id === model);
  const needGb = (choice?.size_gb ?? 0) + (det?.ollama_installed ? 0 : 2);
  const lowDisk = det?.free_gb != null && det.free_gb < needGb + 2;

  return (
    <div className="ai-setup">
      <div className="row between" style={{ marginBottom: 12 }}>
        <div className="row" style={{ gap: 10 }}>
          <span className="ai-setup-orb" aria-hidden><I.assistant /></span>
          <div>
            <b>Set up AI for me</b>
            <div className="small dim">About two minutes of your time; downloads carry on in the background of this page.</div>
          </div>
        </div>
        {compact && phase !== "working" && <button className="btn ghost sm" onClick={() => { setOpen(false); setPhase("choose"); }}>Close</button>}
      </div>

      {detErr && <ErrorNote message={detErr} onRetry={detect} />}
      {!det && !detErr && <div className="row small dim" style={{ gap: 8 }}><Spinner /> Looking at this PC…</div>}

      {det && phase === "choose" && (
        <div className="ai-choices">
          <button className="ai-choice" onClick={() => setPhase("local")}>
            <span className="tag good">Free · private</span>
            <b>Runs on this PC</b>
            <span className="small dim">
              No account, no key, nothing leaves your PC. Uses Ollama with a model picked for your {det.vram_gb ? `${det.vram_gb.toFixed(0)} GB graphics card` : `${det.ram_gb.toFixed(0)} GB of memory`}.
              {det.ollama_running && det.models.length > 0 ? " Ollama is already here, so this can be instant." : ` Downloads about ${(det.recommended.size_gb + (det.ollama_installed ? 0 : 1.9)).toFixed(1)} GB.`}
            </span>
          </button>
          <button className="ai-choice" onClick={() => setPhase("claude")}>
            <span className="tag">Smartest · paid key</span>
            <b>Claude by Anthropic</b>
            <span className="small dim">The most capable answers. Needs an Anthropic API key (you pay Anthropic per use). Your key stays in Windows Credential Manager.</span>
          </button>
        </div>
      )}

      {det && phase === "local" && (
        <div className="stack" style={{ gap: 12 }}>
          <div className="field">
            <label htmlFor="ai-model">Model</label>
            <select id="ai-model" className="select" value={model} onChange={(e) => setModel(e.target.value)}>
              {det.options.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label} · {o.size_gb} GB{det.models.includes(o.id) ? " · already downloaded" : ""}{o.id === det.recommended.id ? " · best for this PC" : ""}
                </option>
              ))}
              {det.models.filter((m) => !det.options.some((o) => o.id === m)).map((m) => (
                <option key={m} value={m}>{m} · already downloaded</option>
              ))}
            </select>
            {model === det.recommended.id && <div className="hint">{det.recommended.why}</div>}
          </div>
          <ul className="ai-steps small">
            <li className={det.ollama_installed ? "done" : ""}>{det.ollama_installed ? `Ollama is installed${det.ollama_version ? ` (${det.ollama_version})` : ""}` : "Install Ollama (official installer from GitHub, checked before it runs, no admin prompt) · 1.9 GB"}</li>
            <li className={already ? "done" : ""}>{already ? `${model} is already downloaded` : `Download ${model}${choice ? ` · ${choice.size_gb} GB` : ""}`}</li>
            <li>Connect NOCTURN to it and test</li>
          </ul>
          {lowDisk && <Note tone="warning">Only {det.free_gb!.toFixed(1)} GB free on the drive models are stored on; this needs about {needGb.toFixed(1)} GB.</Note>}
          {err && <ErrorNote message={err} />}
          <div className="row" style={{ gap: 8 }}>
            <button className="btn primary" onClick={runLocal} disabled={!model}>
              <I.bolt /> {det.ollama_installed && already ? "Use it" : "Set it up"}
            </button>
            <button className="btn ghost" onClick={() => { setErr(null); setPhase("choose"); }}>Back</button>
            <span className="grow" />
            <button className="btn ghost sm" onClick={() => api.aiSetupOpen("ollama").catch(() => {})}>About Ollama</button>
          </div>
        </div>
      )}

      {det && phase === "claude" && (
        <div className="stack" style={{ gap: 12 }}>
          <ol className="ai-steps numbered small">
            <li>
              Open Anthropic's console, sign in, and create a key.{" "}
              <button className="btn sm" onClick={() => api.aiSetupOpen("claude_keys").catch((e) => toast({ tone: "critical", title: "Couldn't open the browser", detail: errorMessage(e) }))}>
                Open the key page
              </button>
            </li>
            <li>Copy the key (it starts with sk-ant-) and paste it here.</li>
          </ol>
          <div className="row">
            <input className="input mono" type="password" autoComplete="off" spellCheck={false} placeholder="sk-ant-…" value={key} onChange={(e) => setKey(e.target.value)} onKeyDown={(e) => e.key === "Enter" && runClaude()} aria-label="Anthropic API key" />
            <button className="btn primary" onClick={runClaude} disabled={!key.trim()}>Save and test</button>
          </div>
          {err && <ErrorNote message={err} />}
          <div><button className="btn ghost" onClick={() => { setErr(null); setPhase("choose"); }}>Back</button></div>
        </div>
      )}

      {phase === "working" && prog && (
        <div className="stack" style={{ gap: 10 }} aria-live="polite">
          <div className="row between">
            <span className="row" style={{ gap: 8 }}><Spinner /> {prog.label}</span>
            {prog.fraction != null && <span className="num small">{Math.round(prog.fraction * 100)}%</span>}
          </div>
          <div className={`progress ai-progress ${prog.fraction == null ? "indeterminate" : ""}`}>
            <span style={{ width: prog.fraction == null ? "30%" : `${Math.max(2, prog.fraction * 100)}%` }} />
          </div>
          {prog.detail && <div className="small dim num">{prog.detail}</div>}
          {prog.stage !== "configure" && (
            <div><button className="btn ghost sm" onClick={() => api.aiSetupCancel()}>Cancel</button></div>
          )}
        </div>
      )}

      {phase === "done" && result && (
        <div className="stack" style={{ gap: 10 }}>
          <Note tone={result.ok ? "good" : "warning"}>{result.message}</Note>
          {!result.ok && <div><button className="btn" onClick={() => setPhase("choose")}>Try again</button></div>}
        </div>
      )}
    </div>
  );
}
