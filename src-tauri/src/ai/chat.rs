//! The agent loop: stream a model turn, run requested read-only tools, feed
//! results back, repeat until the model answers. Every event is forwarded to
//! the UI over a Tauri channel so responses render as they stream.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;

use super::provider::{self, StreamEvent, Usage};
use super::tools::{self, PlanCard};
use super::{prompt, secret_name, store, Block, Message};
use crate::error::{AppError, AppResult};
use crate::security::secrets;
use crate::state::Shared;

const MAX_STEPS: usize = 14;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    Started { conversation_id: String },
    Model { model: String },
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolStart { id: String, name: String },
    ToolResult { id: String, name: String, is_error: bool },
    Plan { plan: PlanCard },
    TurnEnd { stop_reason: String, detail: Option<String>, usage: Usage },
    Error { message: String },
    Done { conversation_id: String, usage: Usage },
}

fn add(total: &mut Usage, u: &Usage) {
    total.input_tokens += u.input_tokens;
    total.output_tokens += u.output_tokens;
    total.cache_read_tokens += u.cache_read_tokens;
    total.cache_write_tokens += u.cache_write_tokens;
}

pub async fn send(
    state: Shared,
    conversation_id: Option<String>,
    text: String,
    meta: Option<Value>,
    events: Channel<ChatEvent>,
) -> AppResult<String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::Invalid("message is empty".into()));
    }
    if text.chars().count() > 20_000 {
        return Err(AppError::Invalid("message is too long (20,000 characters max)".into()));
    }
    let settings = crate::settings::load(&state.db).ai;
    let api_key = secrets::get(secret_name(&settings.provider))?;
    if settings.provider == "anthropic" && api_key.is_none() {
        return Err(AppError::Message(
            "Add your Anthropic API key in Settings › NOCTURN AI to start chatting.".into(),
        ));
    }

    let conv = match conversation_id {
        Some(id) if store::exists(&state.db, &id)? => {
            store::set_model(&state.db, &id, &settings.provider, &settings.model)?;
            id
        }
        Some(id) => return Err(AppError::NotFound(format!("conversation {id}"))),
        None => {
            let title = crate::util::clamp(text.lines().next().unwrap_or("New conversation"), 60);
            store::create(&state.db, &title, &settings.provider, &settings.model)?
        }
    };
    let _ = events.send(ChatEvent::Started { conversation_id: conv.clone() });

    let mut user = Message::user_text(text);
    user.meta = meta;
    store::append(&state.db, &conv, &user)?;

    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut m) = state.ai.cancels.lock() {
        m.insert(conv.clone(), cancel.clone());
    }

    let result = run_loop(&state, &conv, &settings, api_key, &events, &cancel).await;

    if let Ok(mut m) = state.ai.cancels.lock() {
        m.remove(&conv);
    }
    match result {
        Ok(usage) => {
            let _ = events.send(ChatEvent::Done { conversation_id: conv.clone(), usage });
            Ok(conv)
        }
        Err(AppError::Cancelled) => {
            let _ = events.send(ChatEvent::Done { conversation_id: conv.clone(), usage: Usage::default() });
            Ok(conv)
        }
        Err(e) => {
            let msg = e.to_string();
            let _ = events.send(ChatEvent::Error { message: msg.clone() });
            // Record the failure so the transcript explains the gap.
            let _ = store::append(
                &state.db,
                &conv,
                &Message { role: "assistant".into(), blocks: vec![], raw: None, meta: Some(json!({ "error": msg })) },
            );
            let _ = events.send(ChatEvent::Done { conversation_id: conv.clone(), usage: Usage::default() });
            Ok(conv)
        }
    }
}

async fn run_loop(
    state: &Shared,
    conv: &str,
    settings: &crate::settings::AiSettings,
    api_key: Option<String>,
    events: &Channel<ChatEvent>,
    cancel: &Arc<AtomicBool>,
) -> AppResult<Usage> {
    let st = state.clone();
    let info = tauri::async_runtime::spawn_blocking(move || st.system_info(false)).await.ok();
    let system = prompt::build(settings, info.as_ref());
    let tool_defs = tools::definitions();
    let mut total = Usage::default();

    for _step in 0..MAX_STEPS {
        let history: Vec<Message> = store::messages(&state.db, conv)?.into_iter().map(|m| m.message).collect();
        let req = provider::Request {
            settings,
            api_key: api_key.clone(),
            system: system.clone(),
            tools: &tool_defs,
            messages: &history,
        };
        let ev = events.clone();
        let mut on = move |e: StreamEvent| {
            let _ = ev.send(match e {
                StreamEvent::Model(m) => ChatEvent::Model { model: m },
                StreamEvent::TextDelta(t) => ChatEvent::TextDelta { text: t },
                StreamEvent::ThinkingDelta(t) => ChatEvent::ThinkingDelta { text: t },
                StreamEvent::ToolStart { id, name } => ChatEvent::ToolStart { id, name },
            });
        };
        let turn = provider::stream_turn(&req, &mut on, cancel).await?;
        add(&mut total, &turn.usage);

        store::append(
            &state.db,
            conv,
            &Message {
                role: "assistant".into(),
                blocks: turn.blocks.clone(),
                raw: turn.raw.clone(),
                meta: Some(json!({
                    "model": turn.model,
                    "usage": turn.usage,
                    "stop_reason": turn.stop_reason,
                    "stop_detail": turn.stop_detail,
                })),
            },
        )?;
        let _ = events.send(ChatEvent::TurnEnd {
            stop_reason: turn.stop_reason.clone(),
            detail: turn.stop_detail.clone(),
            usage: turn.usage.clone(),
        });

        let tool_uses: Vec<(String, String, Value)> = turn
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::ToolUse { id, name, input } => Some((id.clone(), name.clone(), input.clone())),
                _ => None,
            })
            .collect();

        if turn.stop_reason == "refusal" {
            // Never run tools from a declined turn; answer any dangling calls so
            // the transcript stays valid.
            if !tool_uses.is_empty() {
                let blocks = tool_uses
                    .iter()
                    .map(|(id, _, _)| Block::ToolResult { tool_use_id: id.clone(), content: "not executed".into(), is_error: true })
                    .collect();
                store::append(&state.db, conv, &Message { role: "user".into(), blocks, raw: None, meta: Some(json!({ "hidden": true })) })?;
            }
            return Ok(total);
        }
        if tool_uses.is_empty() {
            if turn.stop_reason == "pause_turn" {
                continue;
            }
            return Ok(total);
        }

        // Run tools. A truncated (max_tokens) turn may carry partial inputs, so
        // its tools are answered with an error instead of being run.
        let truncated = turn.stop_reason == "max_tokens";
        let mut results = Vec::new();
        let mut plans: Vec<PlanCard> = Vec::new();
        for (id, name, input) in tool_uses {
            if cancel.load(Ordering::Relaxed) {
                results.push(Block::ToolResult { tool_use_id: id, content: "cancelled by the user".into(), is_error: true });
                continue;
            }
            if truncated {
                results.push(Block::ToolResult {
                    tool_use_id: id,
                    content: "not executed: the response hit the output limit before this tool call was complete".into(),
                    is_error: true,
                });
                continue;
            }
            if let Some((_, raw)) = turn.invalid_tool_inputs.iter().find(|(tid, _)| tid == &id) {
                let content = json!({ "INVALID_JSON": raw }).to_string();
                let _ = events.send(ChatEvent::ToolResult { id: id.clone(), name: name.clone(), is_error: true });
                results.push(Block::ToolResult { tool_use_id: id, content, is_error: true });
                continue;
            }
            let st = state.clone();
            let (tid, tname, tinput) = (id.clone(), name.clone(), input.clone());
            let out = tauri::async_runtime::spawn_blocking(move || tools::execute(&st, &tid, &tname, &tinput))
                .await
                .map_err(|e| AppError::msg(format!("tool crashed: {e}")))?;
            if let Some(card) = out.plan {
                let _ = events.send(ChatEvent::Plan { plan: card.clone() });
                state.db.audit(
                    "info",
                    "ai",
                    "plan.proposed",
                    Some(json!({ "conversation": conv, "plan": card.plan_id, "tweaks": card.tweaks.iter().map(|t| &t.id).collect::<Vec<_>>(), "startup": card.startup.iter().map(|s| &s.id).collect::<Vec<_>>() })),
                );
                plans.push(card);
            }
            let _ = events.send(ChatEvent::ToolResult { id: id.clone(), name, is_error: out.is_error });
            results.push(Block::ToolResult { tool_use_id: id, content: out.content, is_error: out.is_error });
        }
        let meta = if plans.is_empty() { json!({ "hidden": true }) } else { json!({ "hidden": true, "plans": plans }) };
        store::append(&state.db, conv, &Message { role: "user".into(), blocks: results, raw: None, meta: Some(meta) })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
    }
    Err(AppError::msg("the assistant used too many steps without finishing; try asking a narrower question"))
}

pub fn cancel(state: &Shared, conversation_id: &str) -> bool {
    match state.ai.cancels.lock() {
        Ok(m) => match m.get(conversation_id) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}
