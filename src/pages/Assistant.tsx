// NOCTURN AI: conversation list, streamed agent turns, tool trace, and plan
// cards that route through the shared ApplyFlow. The assistant itself can't
// change anything — a plan only runs after the user approves its preview, and
// the outcome is reported back so the assistant can verify the result.

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import logo from "../logo.png";
import { I } from "../components/Icons";
import { ErrorNote, Note, RiskTag, Spinner } from "../components/ui";
import { api, errorMessage, type ChatEvent, type ConversationSummary, type PlanCard, type Usage } from "../lib/api";
import { useApp } from "../lib/app";
import { pushPart, toTurns, TOOL_LABEL, type ToolRef, type Turn } from "../lib/chat";
import { bytes, relative } from "../lib/format";

// --------------------------------------------------------------- the page

const SUGGESTIONS: { title: string; prompt: string }[] = [
  { title: "Why is my PC slower?", prompt: "My PC has suddenly become slower. Can you check what's going on?" },
  { title: "Optimise for gaming", prompt: "Optimise my PC for gaming." },
  { title: "What's using my RAM?", prompt: "Find what's using my RAM." },
  { title: "Faster boot", prompt: "Windows is taking ages to start. Make it boot faster." },
  { title: "Game stutter", prompt: "My games keep stuttering every few seconds." },
  { title: "Wi-Fi drops", prompt: "My PC randomly disconnects from Wi-Fi." },
];

export function Assistant() {
  const { navigate, requestApply, takePendingPrompt, settings, toast } = useApp();
  const [convs, setConvs] = useState<ConversationSummary[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [live, setLive] = useState<Extract<Turn, { kind: "assistant" }> | null>(null);
  const [pendingUser, setPendingUser] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [input, setInput] = useState("");
  const [usage, setUsage] = useState<Usage | null>(null);
  const [keyOk, setKeyOk] = useState<boolean | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const activeRef = useRef<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  activeRef.current = active;

  const provider = settings?.ai.provider ?? "anthropic";
  const model = settings?.ai.model ?? "";

  const refreshConvs = useCallback(() => api.aiConversations().then(setConvs).catch(() => {}), []);

  const open = useCallback(async (id: string | null) => {
    setActive(id);
    setLoadError(null);
    setUsage(null);
    if (!id) {
      setTurns([]);
      return;
    }
    try {
      setTurns(toTurns(await api.aiConversation(id)));
      stick.current = true;
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    refreshConvs();
    api
      .aiKeyStatus()
      .then((k) => setKeyOk(provider === "anthropic" ? k.anthropic : true))
      .catch(() => setKeyOk(null));
  }, [refreshConvs, provider]);

  // Keep the newest content in view unless the user scrolled up to read.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [turns, live, pendingUser]);

  const send = useCallback(
    async (text: string, meta?: Record<string, unknown>) => {
      const body = text.trim();
      if (!body || busy) return;
      setBusy(true);
      setInput("");
      stick.current = true;
      if (!meta) setPendingUser(body);
      const turn: Extract<Turn, { kind: "assistant" }> = { kind: "assistant", key: "live", parts: [] };
      const tools = new Map<string, ToolRef>();
      setLive({ ...turn });
      const bump = () => setLive({ ...turn, parts: [...turn.parts] });
      let convId = activeRef.current;
      const onEvent = (e: ChatEvent) => {
        switch (e.type) {
          case "started":
            convId = e.conversation_id;
            if (activeRef.current !== convId) {
              activeRef.current = convId;
              setActive(convId);
              refreshConvs();
            }
            break;
          case "model":
            turn.model = e.model;
            break;
          case "text_delta":
            pushPart(turn.parts, { type: "text", text: e.text });
            bump();
            break;
          case "thinking_delta":
            pushPart(turn.parts, { type: "thinking", text: e.text });
            bump();
            break;
          case "tool_start": {
            const t: ToolRef = { id: e.id, name: e.name, state: "running" };
            tools.set(e.id, t);
            pushPart(turn.parts, { type: "tools", tools: [t] });
            bump();
            break;
          }
          case "tool_result": {
            const t = tools.get(e.id);
            if (t) t.state = e.is_error ? "error" : "ok";
            bump();
            break;
          }
          case "plan":
            pushPart(turn.parts, { type: "plan", plan: e.plan });
            bump();
            break;
          case "turn_end":
            setUsage((u) => addUsage(u, e.usage));
            break;
          case "error":
            turn.error = e.message;
            bump();
            break;
          case "done":
            break;
        }
      };
      try {
        convId = await api.aiSend(activeRef.current, body, onEvent, meta);
      } catch (e) {
        // Nothing was sent; give the text back so the user can retry.
        turn.error = errorMessage(e);
        bump();
        setBusy(false);
        setPendingUser(null);
        if (!meta) setInput(body);
        return;
      }
      // The database is the source of truth once the turn finishes.
      try {
        if (convId) setTurns(toTurns(await api.aiConversation(convId)));
        setLive(null);
      } catch {
        /* keep the live view if the reload fails */
      }
      setPendingUser(null);
      setBusy(false);
      refreshConvs();
    },
    [busy, refreshConvs],
  );

  // A prompt handed over from another page starts a fresh conversation.
  useEffect(() => {
    const p = takePendingPrompt();
    if (p) {
      setActive(null);
      activeRef.current = null;
      setTurns([]);
      send(p);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function stop() {
    if (activeRef.current) await api.aiCancel(activeRef.current).catch(() => {});
  }

  async function remove(id: string) {
    try {
      await api.aiDelete(id);
      if (active === id) open(null);
      refreshConvs();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't delete conversation", detail: errorMessage(e) });
    }
  }

  const decided = new Map<string, boolean>();
  for (const t of turns) if (t.kind === "result") decided.set(t.planId, t.approved);

  function review(plan: PlanCard) {
    requestApply({
      tweakIds: plan.tweaks.map((t) => t.id),
      startupDisableIds: plan.startup.map((s) => s.id),
      closeApps: (plan.close_apps ?? []).map((a) => a.name),
      customise: (plan.customise ?? []).map((c) => ({ id: c.id, choice: c.choice })),
      services: (plan.services ?? []).map((v) => ({ name: v.name, start: v.start })),
      openApps: plan.ecosystem?.open_apps ?? [],
      workspace: plan.ecosystem?.workspace ?? null,
      music: plan.ecosystem?.music ?? null,
      source: "ai",
      label: plan.title,
      title: plan.title,
      description: `${plan.summary} Nothing changes until you approve.`,
      onDone: (o) => {
        const text = o.approved
          ? `Plan result for "${plan.title}": ${o.summary} Please verify whether this helped.`
          : `Plan result for "${plan.title}": the user declined the plan. Nothing was changed.`;
        send(text, { kind: "plan_result", plan_id: plan.plan_id, approved: o.approved, tx_ids: o.txIds });
      },
    });
  }

  function decline(plan: PlanCard) {
    send(`Plan result for "${plan.title}": the user declined the plan. Nothing was changed.`, { kind: "plan_result", plan_id: plan.plan_id, approved: false });
  }

  const empty = !active && turns.length === 0 && !live && !pendingUser;

  return (
    <div className="assistant">
      <aside className="conv-list" aria-label="Conversations">
        <div className="head">
          <span className="small muted">Conversations</span>
          <button className="btn ghost sm" onClick={() => open(null)} disabled={busy} title="New conversation">
            <I.plus /> New
          </button>
        </div>
        <div className="items">
          {convs.length === 0 && <div className="small dim" style={{ padding: "8px 10px" }}>No conversations yet.</div>}
          {convs.map((c) => (
            <div key={c.id} className={`conv-item ${active === c.id ? "active" : ""}`} role="button" tabIndex={0} onClick={() => !busy && open(c.id)} onKeyDown={(e) => e.key === "Enter" && !busy && open(c.id)} title={`${c.title}\n${relative(c.updated_at)} · ${c.model}`}>
              <span className="truncate" style={{ flex: 1 }}>{c.title}</span>
              <button className="del" aria-label={`Delete ${c.title}`} onClick={(e) => { e.stopPropagation(); remove(c.id); }} disabled={busy}>
                <I.x style={{ width: 12, height: 12 }} />
              </button>
            </div>
          ))}
        </div>
      </aside>

      <section className="chat">
        <div className="chat-scroll" ref={scrollRef} onScroll={(e) => { const el = e.currentTarget; stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60; }}>
          <div className="chat-inner">
            {keyOk === false && (
              <Note tone="warning">
                <div className="row between" style={{ gap: 14 }}>
                  <span>NOCTURN AI needs an API key before it can answer. Keys are stored in Windows Credential Manager, never in the app's files.</span>
                  <button className="btn sm" onClick={() => navigate("settings")}>Open Settings</button>
                </div>
              </Note>
            )}
            {settings && !settings.ai.allow_system_context && (
              <Note>System access is off in Settings, so the assistant can't inspect this PC and will ask you for details instead.</Note>
            )}
            {loadError && <ErrorNote message={loadError} onRetry={() => open(active)} />}

            {empty && (
              <div className="welcome">
                <img src={logo} alt="" />
                <h2>What's going on with your PC?</h2>
                <p>Describe a problem in your own words. NOCTURN AI looks at this PC's real data before answering, separates confirmed causes from possible ones, and never changes anything without your approval.</p>
                <div className="suggest-grid">
                  {SUGGESTIONS.map((s) => (
                    <button key={s.title} className="suggest" onClick={() => send(s.prompt)} disabled={busy}>
                      <b>{s.title}</b>
                      {s.prompt}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {turns.map((t) => (
              <TurnView key={t.key} turn={t} decided={decided} busy={busy} onReview={review} onDecline={decline} />
            ))}
            {pendingUser && <TurnView turn={{ kind: "user", key: "pending", text: pendingUser }} decided={decided} busy onReview={review} onDecline={decline} />}
            {live && <TurnView turn={live} decided={decided} busy streaming onReview={review} onDecline={decline} />}
          </div>
        </div>

        <div className="composer-wrap">
          <form
            className="composer"
            onSubmit={(e) => {
              e.preventDefault();
              send(input);
            }}
          >
            <textarea
              value={input}
              rows={1}
              onChange={(e) => {
                setInput(e.target.value);
                e.target.style.height = "auto";
                e.target.style.height = `${Math.min(180, e.target.scrollHeight)}px`;
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  send(input);
                }
              }}
              placeholder={busy ? "NOCTURN AI is working…" : "Describe a problem, or ask about your PC"}
              aria-label="Message NOCTURN AI"
              maxLength={20000}
            />
            {busy ? (
              <button type="button" className="btn icon" onClick={stop} aria-label="Stop">
                <I.stop />
              </button>
            ) : (
              <button type="submit" className="btn primary icon" disabled={!input.trim()} aria-label="Send">
                <I.send />
              </button>
            )}
          </form>
          <div className="composer-meta">
            <span>{provider === "anthropic" ? "Claude" : "OpenAI-compatible"} · {model || "no model set"}</span>
            <span className="num">
              {usage ? `This exchange: ${fmtTokens(usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens)} in · ${fmtTokens(usage.output_tokens)} out` : "Enter to send · Shift+Enter for a new line"}
            </span>
          </div>
        </div>
      </section>
    </div>
  );
}

function addUsage(a: Usage | null, b: Usage): Usage {
  return {
    input_tokens: (a?.input_tokens ?? 0) + b.input_tokens,
    output_tokens: (a?.output_tokens ?? 0) + b.output_tokens,
    cache_read_tokens: (a?.cache_read_tokens ?? 0) + b.cache_read_tokens,
    cache_write_tokens: (a?.cache_write_tokens ?? 0) + b.cache_write_tokens,
  };
}

function fmtTokens(n: number) {
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k tokens` : `${n} tokens`;
}

// ------------------------------------------------------------- rendering

function TurnView({ turn, decided, busy, streaming, onReview, onDecline }: { turn: Turn; decided: Map<string, boolean>; busy: boolean; streaming?: boolean; onReview: (p: PlanCard) => void; onDecline: (p: PlanCard) => void }) {
  if (turn.kind === "user")
    return (
      <div className="msg user">
        <div className="bubble selectable">{turn.text}</div>
      </div>
    );
  if (turn.kind === "result")
    return (
      <div className="row small dim" style={{ justifyContent: "center", gap: 8 }}>
        {turn.approved ? <I.check style={{ width: 13, height: 13 }} /> : <I.x style={{ width: 13, height: 13 }} />}
        {turn.approved ? "Plan applied — results sent to NOCTURN AI for verification" : "Plan declined — nothing was changed"}
      </div>
    );
  const lastText = [...turn.parts].reverse().find((p) => p.type === "text");
  return (
    <div className="msg assistant">
      <div className="who">
        <img src={logo} alt="" /> NOCTURN AI
        {streaming && turn.parts.length === 0 && <Spinner />}
      </div>
      {turn.parts.map((p, i) => {
        if (p.type === "text") return <Markdown key={i} text={p.text} cursor={streaming && p === lastText && i === turn.parts.length - 1} />;
        if (p.type === "thinking")
          return (
            <details key={i} className="thinking">
              <summary>
                <I.assistant style={{ width: 12, height: 12 }} /> Reasoning
              </summary>
              <div className="body selectable">{p.text}</div>
            </details>
          );
        if (p.type === "tools")
          return (
            <div key={i} className="tool-trace">
              {p.tools.map((t) => (
                <span key={t.id} className={`tool-chip ${t.state === "error" ? "err" : ""}`} title={t.name}>
                  {t.state === "running" ? <Spinner /> : t.state === "error" ? <I.x /> : <I.check />}
                  {TOOL_LABEL[t.name] ?? t.name}
                </span>
              ))}
            </div>
          );
        return <PlanView key={i} plan={p.plan} decision={decided.get(p.plan.plan_id)} busy={busy} onReview={onReview} onDecline={onDecline} />;
      })}
      {turn.error && <ErrorNote message={turn.error} />}
    </div>
  );
}

function Markdown({ text, cursor }: { text: string; cursor?: boolean }) {
  return (
    <div className="prose selectable">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // Links never navigate the app window; clicking copies the address.
          a: ({ href, children }) => (
            <a
              href={href}
              title={`${href} — click to copy`}
              onClick={(e) => {
                e.preventDefault();
                if (href) navigator.clipboard?.writeText(href).catch(() => {});
              }}
            >
              {children}
            </a>
          ),
          pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
          img: ({ alt }) => <span className="dim">[image: {alt}]</span>,
        }}
      >
        {text}
      </ReactMarkdown>
      {cursor && <span className="cursor-blink" />}
    </div>
  );
}

function CodeBlock({ children }: { children?: ReactNode }) {
  const ref = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);
  return (
    <pre ref={ref}>
      <button
        type="button"
        className="code-note btn ghost sm"
        style={{ height: 20, padding: "0 6px" }}
        onClick={() => {
          navigator.clipboard?.writeText(ref.current?.querySelector("code")?.textContent ?? "").then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          });
        }}
      >
        {copied ? "Copied" : "Copy · run it yourself"}
      </button>
      {children}
    </pre>
  );
}

const CONFIDENCE: Record<string, string> = { high: "High confidence", medium: "Medium confidence", low: "Low confidence" };

function PlanView({ plan, decision, busy, onReview, onDecline }: { plan: PlanCard; decision: boolean | undefined; busy: boolean; onReview: (p: PlanCard) => void; onDecline: (p: PlanCard) => void }) {
  const apps = plan.close_apps ?? [];
  const custom = (plan.customise ?? []).filter((c) => c.will_change);
  const svc = (plan.services ?? []).filter((v) => v.will_change);
  const eco = plan.ecosystem;
  const ecoCount = (eco?.open_apps.length ?? 0) + (eco?.workspace ? 1 : 0) + (eco?.music ? 1 : 0);
  const changes = (plan.preview?.changes ?? 0) + plan.startup.length + apps.length + custom.length + svc.length + ecoCount;
  return (
    <div className="plan">
      <div className="ph">
        <I.tweak style={{ width: 18, height: 18, color: "var(--moon-2)", marginTop: 2 }} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="kicker">Proposed plan · {CONFIDENCE[plan.confidence] ?? plan.confidence}</div>
          <h4>{plan.title}</h4>
        </div>
        {decision !== undefined && <span className={`tag ${decision ? "good" : ""}`}>{decision ? "Approved" : "Declined"}</span>}
      </div>
      <div className="pb">
        <div>{plan.summary}</div>
        {plan.expected_effect && (
          <div>
            <span className="dim">Expected effect: </span>
            {plan.expected_effect}
          </div>
        )}
        {plan.verification && (
          <div>
            <span className="dim">How to verify: </span>
            {plan.verification}
          </div>
        )}
      </div>
      <div className="items">
        {plan.tweaks.map((t) => (
          <div key={t.id} className="item">
            <span style={{ flex: 1, minWidth: 0 }}>
              {t.title}
              {t.caveat && <div className="small dim">{t.caveat}</div>}
            </span>
            <RiskTag risk={t.risk} />
            {t.requires_admin && (
              <span className="tag">
                <I.lock /> Admin
              </span>
            )}
            {!t.reversible && <span className="tag warning">One-time</span>}
          </div>
        ))}
        {apps.map((a) => (
          <div key={a.name} className="item">
            <span style={{ flex: 1 }}>
              Close app: {a.name}
              <span className="dim"> · {a.instances} process{a.instances === 1 ? "" : "es"} · {bytes(a.memory_bytes)}</span>
            </span>
            <span className="tag warning">Unsaved work is lost</span>
          </div>
        ))}
        {eco?.workspace && (
          <div className="item"><span style={{ flex: 1 }}>Switch to workspace: {eco.workspace.name}{eco.workspace.launches ? <span className="dim"> · opens {eco.workspace.launches} app(s)</span> : null}</span></div>
        )}
        {eco?.open_apps.map((a) => (
          <div key={a.id} className="item"><span style={{ flex: 1 }}>Open {a.name}<span className="dim"> · in {a.opens}</span></span></div>
        ))}
        {eco?.music && (
          <div className="item"><span style={{ flex: 1 }}>Play {eco.music.label}<span className="dim"> · {eco.music.track_ids.length} songs{eco.music.shuffle ? ", shuffled" : ""}</span></span></div>
        )}
        {custom.map((c) => (
          <div key={c.id} className="item">
            <span style={{ flex: 1 }}>
              {c.title}: {c.proposed}
              <span className="dim"> · now {c.current}</span>
            </span>
          </div>
        ))}
        {svc.map((v) => (
          <div key={v.name} className="item">
            <span style={{ flex: 1 }}>
              Service {v.display_name}: {v.current} → {v.proposed}
              {v.warnings.map((w) => <div key={w} className="small dim">{w}</div>)}
            </span>
            <span className="tag"><I.lock /> Admin</span>
          </div>
        ))}
        {plan.startup.map((s) => (
          <div key={s.id} className="item">
            <span style={{ flex: 1 }}>Disable startup app: {s.name}</span>
            {s.requires_admin && (
              <span className="tag">
                <I.lock /> Admin
              </span>
            )}
          </div>
        ))}
      </div>
      {decision === undefined && (
        <div className="pf">
          <span className="small muted" style={{ flex: 1 }}>
            {plan.preview ? `${plan.preview.changes} setting change(s)` : ""}
            {plan.preview && plan.startup.length ? " · " : ""}
            {plan.startup.length ? `${plan.startup.length} startup app(s)` : ""}
            {apps.length > 0 && (plan.preview || plan.startup.length > 0) ? " · " : ""}
            {apps.length > 0 ? `${apps.length} app(s) to close` : ""}
            {custom.length > 0 ? ` · ${custom.length} Windows setting(s)` : ""}
            {svc.length > 0 ? ` · ${svc.length} service(s)` : ""}
            {changes === 0 && "Everything here is already configured"}
          </span>
          <button className="btn ghost sm" disabled={busy} onClick={() => onDecline(plan)}>
            Decline
          </button>
          <button className="btn primary sm" disabled={busy} onClick={() => onReview(plan)}>
            Review changes <I.arrow />
          </button>
        </div>
      )}
    </div>
  );
}
