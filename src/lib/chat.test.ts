import { describe, expect, it } from "vitest";
import type { PlanCard, StoredMessage } from "./api";
import { pushPart, toTurns, type Part } from "./chat";

const at = "2026-11-05T00:00:00Z";
let id = 0;
const msg = (role: "user" | "assistant", blocks: StoredMessage["blocks"], meta?: StoredMessage["meta"]): StoredMessage => ({ id: ++id, created_at: at, role, blocks, meta: meta ?? null });

const plan: PlanCard = {
  plan_id: "toolu_2",
  title: "Gaming tune-up",
  summary: "Turn on Game Mode.",
  confidence: "high",
  expected_effect: "Fewer background interruptions",
  verification: null,
  tweaks: [{ id: "gaming.game_mode", title: "Game Mode", risk: "safe", requires_admin: false, reversible: true, caveat: null }],
  startup: [],
  preview: null,
};

describe("pushPart", () => {
  it("merges consecutive text and keeps different kinds separate", () => {
    const parts: Part[] = [];
    pushPart(parts, { type: "text", text: "Hel" });
    pushPart(parts, { type: "text", text: "lo" });
    pushPart(parts, { type: "thinking", text: "hmm" });
    pushPart(parts, { type: "text", text: "!" });
    expect(parts).toEqual([
      { type: "text", text: "Hello" },
      { type: "thinking", text: "hmm" },
      { type: "text", text: "!" },
    ]);
  });

  it("does not alias the caller's part objects", () => {
    const parts: Part[] = [];
    const p = { type: "text" as const, text: "a" };
    pushPart(parts, p);
    pushPart(parts, { type: "text", text: "b" });
    expect(p.text).toBe("a");
  });
});

describe("toTurns", () => {
  it("folds an agent loop into one assistant turn with tool states and plans", () => {
    const turns = toTurns([
      msg("user", [{ type: "text", text: "Optimise for gaming" }]),
      msg("assistant", [
        { type: "text", text: "Let me look." },
        { type: "tool_use", id: "toolu_1", name: "get_live_metrics", input: {} },
      ]),
      msg("user", [{ type: "tool_result", tool_use_id: "toolu_1", content: "{}", is_error: true }], { hidden: true }),
      msg("assistant", [{ type: "tool_use", id: "toolu_2", name: "propose_plan", input: {} }]),
      msg("user", [{ type: "tool_result", tool_use_id: "toolu_2", content: "{}", is_error: false }], { hidden: true, plans: [plan] }),
      msg("assistant", [{ type: "text", text: "Here's a plan." }], { model: "claude-opus-5-5" }),
    ]);
    expect(turns).toHaveLength(2);
    expect(turns[0]).toMatchObject({ kind: "user", text: "Optimise for gaming" });
    const a = turns[1];
    if (a.kind !== "assistant") throw new Error("expected assistant turn");
    expect(a.model).toBe("claude-opus-5-5");
    expect(a.parts.map((p) => p.type)).toEqual(["text", "tools", "plan", "text"]);
    const tools = a.parts[1];
    if (tools.type !== "tools") throw new Error("expected tools");
    expect(tools.tools.map((t) => [t.name, t.state])).toEqual([
      ["get_live_metrics", "error"],
      ["propose_plan", "ok"],
    ]);
  });

  it("renders plan results as their own turn using the stored decision", () => {
    const turns = toTurns([
      msg("user", [{ type: "text", text: 'Plan result for "X": the user declined the plan.' }], { kind: "plan_result", plan_id: "p1" }),
      msg("user", [{ type: "text", text: 'Plan result for "Y": Applied 2 change(s).' }], { kind: "plan_result", plan_id: "p2", approved: true } as StoredMessage["meta"]),
    ]);
    expect(turns.map((t) => t.kind === "result" && [t.planId, t.approved])).toEqual([
      ["p1", false],
      ["p2", true],
    ]);
  });

  it("surfaces stored errors on the assistant turn", () => {
    const turns = toTurns([msg("user", [{ type: "text", text: "hi" }]), msg("assistant", [], { error: "invalid API key" })]);
    expect(turns[1]).toMatchObject({ kind: "assistant", error: "invalid API key", parts: [] });
  });
});
