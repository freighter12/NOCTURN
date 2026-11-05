// Chat view model: folds stored messages and live stream events into display
// turns. Pure functions, unit-tested in chat.test.ts.

import type { PlanCard, StoredMessage } from "./api";

export type ToolRef = { id: string; name: string; state: "running" | "ok" | "error" };
export type Part =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tools"; tools: ToolRef[] }
  | { type: "plan"; plan: PlanCard };
export type Turn =
  | { kind: "user"; key: string; text: string }
  | { kind: "result"; key: string; text: string; planId: string; approved: boolean }
  | { kind: "assistant"; key: string; parts: Part[]; error?: string; model?: string };

export const TOOL_LABEL: Record<string, string> = {
  get_system_overview: "Read system overview",
  get_live_metrics: "Read live metrics",
  list_top_processes: "Checked running processes",
  list_startup_apps: "Checked startup apps",
  get_storage: "Checked storage",
  get_network_diagnostics: "Ran network diagnostics",
  get_recent_system_errors: "Read the event log",
  get_drivers: "Checked drivers",
  get_health_report: "Ran health check",
  list_optimisations: "Looked up optimisations",
  get_optimisation_history: "Read optimisation history",
  get_power_plans: "Checked power plans",
  propose_plan: "Prepared a plan",
};

export function pushPart(parts: Part[], p: Part) {
  const last = parts[parts.length - 1];
  if (p.type === "text" && last?.type === "text") last.text += p.text;
  else if (p.type === "thinking" && last?.type === "thinking") last.text += p.text;
  else if (p.type === "tools" && last?.type === "tools") last.tools.push(...p.tools);
  else parts.push(p.type === "tools" ? { type: "tools", tools: [...p.tools] } : { ...p });
}

/** Fold stored messages into display turns: consecutive assistant messages and
 *  the hidden tool-result messages between them become one assistant turn. */
export function toTurns(msgs: StoredMessage[]): Turn[] {
  const turns: Turn[] = [];
  let cur: Extract<Turn, { kind: "assistant" }> | null = null;
  const toolState = new Map<string, ToolRef>();
  for (const m of msgs) {
    const meta = m.meta ?? {};
    if (m.role === "user" && meta.hidden) {
      for (const b of m.blocks) {
        if (b.type === "tool_result") {
          const t = toolState.get(b.tool_use_id);
          if (t) t.state = b.is_error ? "error" : "ok";
        }
      }
      if (cur && meta.plans) for (const plan of meta.plans) pushPart(cur.parts, { type: "plan", plan });
      continue;
    }
    if (m.role === "user") {
      cur = null;
      const text = m.blocks.filter((b) => b.type === "text").map((b) => (b as { text: string }).text).join("\n");
      if (meta.kind === "plan_result" && meta.plan_id) {
        const approved = (meta as { approved?: boolean }).approved;
        turns.push({ kind: "result", key: `m${m.id}`, text, planId: meta.plan_id, approved: approved ?? !/declined/i.test(text) });
      } else turns.push({ kind: "user", key: `m${m.id}`, text });
      continue;
    }
    if (!cur) {
      cur = { kind: "assistant", key: `m${m.id}`, parts: [] };
      turns.push(cur);
    }
    if (meta.model) cur.model = meta.model;
    if (meta.error) cur.error = meta.error;
    for (const b of m.blocks) {
      if (b.type === "text" && b.text) pushPart(cur.parts, { type: "text", text: b.text });
      else if (b.type === "thinking" && b.text) pushPart(cur.parts, { type: "thinking", text: b.text });
      else if (b.type === "tool_use") {
        const t: ToolRef = { id: b.id, name: b.name, state: "ok" };
        toolState.set(b.id, t);
        pushPart(cur.parts, { type: "tools", tools: [t] });
      }
    }
  }
  return turns;
}

