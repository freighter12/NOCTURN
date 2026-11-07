// Settings: AI provider/model/key (keys go to Windows Credential Manager via
// the backend and are never readable from the UI), safety defaults, and
// app information.

import { useEffect, useState, type ReactNode } from "react";
import { I } from "../components/Icons";
import { PrivacyPanel } from "../components/Privacy";
import { ErrorNote, Loading, Note, PageHead, Panel, Spinner, Toggle } from "../components/ui";
import { api, errorMessage, type AiSettings, type AppInfo, type AppSettings, type ConnectionTest } from "../lib/api";
import { useApp } from "../lib/app";

const CLAUDE_MODELS = [
  { id: "claude-opus-5-5", label: "Claude Opus 5.5 — most capable" },
  { id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5 — fast and capable" },
  { id: "claude-haiku-5-5", label: "Claude Haiku 5.5 — fastest, lowest cost" },
];
const EFFORT = ["low", "medium", "high", "xhigh", "max"];

export function Settings() {
  const { settings, setSettings, toast } = useApp();
  const [draft, setDraft] = useState<AppSettings | null>(settings);
  const [saving, setSaving] = useState(false);
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    if (settings && !draft) setDraft(settings);
  }, [settings, draft]);
  useEffect(() => {
    api.appInfo().then(setInfo).catch(() => {});
  }, []);

  if (!draft) return <div className="page"><Loading label="Loading settings…" /></div>;
  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);

  /** Persist `next`. Behaviour toggles save on their own without touching
   *  unsaved AI edits, so only the changed fields are merged into the draft. */
  async function save(next: AppSettings, mergeDraft?: (d: AppSettings) => AppSettings) {
    setSaving(true);
    try {
      const saved = await api.saveSettings(next);
      setSettings(saved);
      setDraft((d) => (mergeDraft && d ? mergeDraft(d) : saved));
      toast({ tone: "good", title: "Settings saved" });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save settings", detail: errorMessage(e) });
    } finally {
      setSaving(false);
    }
  }

  const setAi = (patch: Partial<AiSettings>) => setDraft({ ...draft, ai: { ...draft.ai, ...patch } });
  const setFlag = (patch: Partial<AppSettings>) => {
    if (!settings) return;
    save({ ...settings, ...patch }, (d) => ({ ...d, ...patch }));
  };

  return (
    <div className="page">
      <PageHead eyebrow="Settings" title="Settings" desc="NOCTURN AI, how changes are applied, and your privacy. Looks and layout are in Customise." />

      <div className="grid" style={{ gridTemplateColumns: "minmax(0, 1.5fr) minmax(0, 1fr)", alignItems: "start" }}>
        <div className="stack" style={{ gap: 14 }}>
          <AiPanel ai={draft.ai} setAi={setAi} dirty={dirty} saving={saving} onSave={() => save(draft)} onReset={() => setDraft(settings)} />
          <PrivacyPanel />
        </div>

        <div className="stack" style={{ gap: 14 }}>
          <Panel title="Applying changes">
            <div className="list">
              <Row title="Offer a restore point" desc="Pre-tick “Create a Windows restore point” before changes that need administrator rights.">
                <Toggle on={draft.restore_point_default} onChange={(v) => setFlag({ restore_point_default: v })} label="Offer a restore point" disabled={saving} />
              </Row>
              <Row title="Confirm Advanced changes" desc="Require an extra acknowledgement before applying anything marked Advanced.">
                <Toggle on={draft.confirm_advanced} onChange={(v) => setFlag({ confirm_advanced: v })} label="Confirm Advanced changes" disabled={saving} />
              </Row>
              <Row title="Reduce motion" desc="Turn off non-essential animation.">
                <Toggle on={draft.reduce_motion} onChange={(v) => setFlag({ reduce_motion: v })} label="Reduce motion" disabled={saving} />
              </Row>
            </div>
          </Panel>

          <Panel title="In the background">
            <div className="list">
              <Row title="Keep running in the tray" desc="The close button hides NOCTURN to the system tray so scheduled checks and app profiles keep working. Quit from the tray menu. Off: closing quits.">
                <Toggle on={draft.close_to_tray} onChange={(v) => setFlag({ close_to_tray: v })} label="Keep running in the tray" disabled={saving} />
              </Row>
              <Row title="Desktop notifications" desc="Windows notifications when a scheduled check finds something new (a crash, a device problem, low disk space) while NOCTURN isn't in front.">
                <Toggle on={draft.desktop_notifications} onChange={(v) => setFlag({ desktop_notifications: v })} label="Desktop notifications" disabled={saving} />
              </Row>
            </div>
          </Panel>

          <Panel title="About NOCTURN">
            <dl className="kv">
              <dt>Version</dt>
              <dd>{info?.version ?? "—"}</dd>
              <dt>Running as</dt>
              <dd>{info ? (info.is_elevated ? "Administrator" : "Standard user") : "—"}</dd>
              <dt>Data folder</dt>
              <dd className="mono selectable">{info?.data_dir ?? "—"}</dd>
              <dt>Logs</dt>
              <dd className="mono selectable">{info?.log_dir ?? "—"}</dd>
            </dl>
            <div className="small dim mt-2">
              Conversations, the change journal and settings are stored locally in the data folder. API keys are stored separately in Windows Credential Manager.
            </div>
          </Panel>
        </div>
      </div>
    </div>
  );
}

function Row({ title, desc, children }: { title: string; desc: string; children: ReactNode }) {
  return (
    <div className="list-item" style={{ alignItems: "center" }}>
      <div className="body">
        <div className="title">{title}</div>
        <div className="desc">{desc}</div>
      </div>
      {children}
    </div>
  );
}

function AiPanel({ ai, setAi, dirty, saving, onSave, onReset }: { ai: AiSettings; setAi: (p: Partial<AiSettings>) => void; dirty: boolean; saving: boolean; onSave: () => void; onReset: () => void }) {
  const { toast } = useApp();
  const [keys, setKeys] = useState<{ anthropic: boolean; openai_compatible: boolean } | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [keyBusy, setKeyBusy] = useState(false);
  const [test, setTest] = useState<ConnectionTest | null>(null);
  const [testing, setTesting] = useState(false);
  const [testError, setTestError] = useState<string | null>(null);

  const loadKeys = () => api.aiKeyStatus().then(setKeys).catch(() => {});
  useEffect(() => {
    loadKeys();
  }, []);
  useEffect(() => {
    setTest(null);
    setTestError(null);
  }, [ai.provider]);

  const hasKey = keys ? keys[ai.provider] : false;
  const isClaude = ai.provider === "anthropic";

  async function storeKey(value: string | null) {
    setKeyBusy(true);
    try {
      await api.aiSetKey(ai.provider, value);
      setKeyInput("");
      await loadKeys();
      toast({ tone: "good", title: value ? "API key saved to Windows Credential Manager" : "API key removed" });
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't store the key", detail: errorMessage(e) });
    } finally {
      setKeyBusy(false);
    }
  }

  async function runTest() {
    setTesting(true);
    setTestError(null);
    setTest(null);
    try {
      setTest(await api.aiTest());
    } catch (e) {
      setTestError(errorMessage(e));
    } finally {
      setTesting(false);
    }
  }

  return (
    <Panel
      lit
      title="NOCTURN AI"
      sub="Choose the AI provider and model. Your key never leaves this PC except to the provider you choose."
      actions={
        dirty && (
          <span className="row" style={{ gap: 6 }}>
            <button className="btn ghost sm" onClick={onReset} disabled={saving}>Discard</button>
            <button className="btn primary sm" onClick={onSave} disabled={saving}>{saving ? <Spinner /> : null} Save</button>
          </span>
        )
      }
    >
      <div className="stack" style={{ gap: 16 }}>
        <div className="field">
          <label htmlFor="provider">Provider</label>
          <select id="provider" className="select" value={ai.provider} onChange={(e) => setAi({ provider: e.target.value as AiSettings["provider"], model: e.target.value === "anthropic" ? CLAUDE_MODELS[0].id : "" })}>
            <option value="anthropic">Anthropic (Claude)</option>
            <option value="openai_compatible">OpenAI-compatible (OpenAI, OpenRouter, LM Studio, Ollama…)</option>
          </select>
        </div>

        {!isClaude && (
          <div className="field">
            <label htmlFor="base">Base URL</label>
            <input id="base" className="input mono" value={ai.base_url} onChange={(e) => setAi({ base_url: e.target.value })} placeholder="https://api.openai.com/v1" spellCheck={false} />
            <div className="hint">Must use https, unless it points at this PC (http://localhost…) for a local model.</div>
          </div>
        )}

        <div className="field">
          <label htmlFor="model">Model</label>
          {isClaude ? (
            <select id="model" className="select" value={CLAUDE_MODELS.some((m) => m.id === ai.model) ? ai.model : "__custom"} onChange={(e) => e.target.value !== "__custom" && setAi({ model: e.target.value })}>
              {CLAUDE_MODELS.map((m) => (
                <option key={m.id} value={m.id}>{m.label}</option>
              ))}
              {!CLAUDE_MODELS.some((m) => m.id === ai.model) && <option value="__custom">{ai.model || "Custom"}</option>}
            </select>
          ) : (
            <>
              <input id="model" className="input mono" list="model-list" value={ai.model} onChange={(e) => setAi({ model: e.target.value })} placeholder="e.g. a model ID from your provider" spellCheck={false} />
              <datalist id="model-list">{test?.models.map((m) => <option key={m} value={m} />)}</datalist>
              <div className="hint">Test the connection to load the provider's model list. The model must support tool calling.</div>
            </>
          )}
        </div>

        {isClaude && (
          <div className="field">
            <label htmlFor="effort">Thinking effort</label>
            <select id="effort" className="select" value={ai.effort} onChange={(e) => setAi({ effort: e.target.value })}>
              {EFFORT.map((x) => (
                <option key={x} value={x}>{x[0].toUpperCase() + x.slice(1)}{x === "medium" ? " (recommended)" : ""}</option>
              ))}
            </select>
            <div className="hint">Higher effort investigates more thoroughly but responds more slowly and uses more tokens.</div>
          </div>
        )}

        <div className="field">
          <label htmlFor="maxtok">Maximum response length (tokens)</label>
          <input id="maxtok" className="input num" type="number" min={1024} max={128000} step={1024} value={ai.max_tokens} onChange={(e) => setAi({ max_tokens: Math.max(1024, Math.min(128000, Number(e.target.value) || 1024)) })} />
        </div>

        <hr className="divider" style={{ margin: 0 }} />

        <div className="field">
          <label htmlFor="key">API key {isClaude ? "(required)" : "(optional for local servers)"}</label>
          <div className="row">
            <input
              id="key"
              className="input mono"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              placeholder={hasKey ? "•••••••••••• stored — enter a new key to replace it" : isClaude ? "sk-ant-…" : "Provider API key"}
            />
            <button className="btn" disabled={!keyInput.trim() || keyBusy} onClick={() => storeKey(keyInput.trim())}>
              {keyBusy ? <Spinner /> : <I.lock />} Save key
            </button>
            {hasKey && (
              <button className="btn ghost" disabled={keyBusy} onClick={() => storeKey(null)}>
                Remove
              </button>
            )}
          </div>
          <div className="hint row" style={{ gap: 6 }}>
            <span className="status-dot" style={{ background: hasKey ? "var(--good)" : "var(--ink-3)" }} />
            {hasKey ? "A key is stored in Windows Credential Manager. It can't be displayed again." : "No key stored for this provider."}
          </div>
        </div>

        <div className="row">
          <button className="btn" onClick={runTest} disabled={testing || dirty}>
            {testing ? <Spinner /> : <I.bolt />} Test connection
          </button>
          {dirty && <span className="small dim">Save changes before testing.</span>}
        </div>
        {testError && <ErrorNote message={testError} />}
        {test && <Note tone={test.ok ? "good" : "critical"}>{test.message}</Note>}

        <hr className="divider" style={{ margin: 0 }} />

        <div className="list-item" style={{ alignItems: "center", borderTop: 0, padding: 0 }}>
          <div className="body">
            <div className="title">Let the assistant inspect this PC</div>
            <div className="desc">Read-only access to hardware, processes, startup apps, event log, drivers, network and NOCTURN history. It can never change anything without your approval.</div>
          </div>
          <Toggle on={ai.allow_system_context} onChange={(v) => setAi({ allow_system_context: v })} label="Let the assistant inspect this PC" />
        </div>

        <div className="field">
          <label htmlFor="ci">Custom instructions</label>
          <textarea id="ci" className="textarea" rows={3} maxLength={4000} value={ai.custom_instructions} onChange={(e) => setAi({ custom_instructions: e.target.value })} placeholder="e.g. I'm comfortable with technical detail. I mainly play competitive shooters." />
          <div className="hint">Added to the assistant's instructions. They can't override NOCTURN's safety rules.</div>
        </div>
      </div>
    </Panel>
  );
}
