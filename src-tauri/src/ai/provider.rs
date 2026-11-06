//! Streaming model providers.
//!
//! **Anthropic** uses the native Messages API over HTTPS (Rust has no official
//! Anthropic SDK, so this is the documented raw-HTTP path): adaptive thinking
//! with summarised display, `output_config.effort`, server-side refusal
//! fallbacks, prompt caching, and eager tool-input streaming with strict
//! client-side JSON validation.
//!
//! **OpenAI-compatible** speaks `/chat/completions` for users who configure
//! another provider or a local model server.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};

use super::{Block, Message};
use crate::error::{AppError, AppResult};
use crate::settings::AiSettings;

pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models";
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Model(String),
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart { id: String, name: String },
}

#[derive(Debug, Clone)]
pub struct Turn {
    pub blocks: Vec<Block>,
    pub raw: Option<Value>,
    pub stop_reason: String,
    pub stop_detail: Option<String>,
    pub usage: Usage,
    pub model: String,
    /// Tool calls whose streamed input was not valid JSON (id → raw text).
    pub invalid_tool_inputs: Vec<(String, String)>,
}

pub struct Request<'a> {
    pub settings: &'a AiSettings,
    pub api_key: Option<String>,
    pub system: String,
    pub tools: &'a [ToolDef],
    pub messages: &'a [Message],
}

fn client() -> AppResult<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .user_agent(concat!("NOCTURN/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Allow https anywhere, http only for local model servers.
pub fn validate_base_url(url: &str) -> AppResult<String> {
    let u = url.trim().trim_end_matches('/');
    let ok = u.starts_with("https://")
        || u.starts_with("http://localhost")
        || u.starts_with("http://127.0.0.1")
        || u.starts_with("http://[::1]");
    if !ok || u.contains(char::is_whitespace) {
        return Err(AppError::Invalid(
            "the API address must start with https:// (http is only allowed for localhost)".into(),
        ));
    }
    Ok(u.to_string())
}

/// Models that accept adaptive thinking + `output_config.effort`.
fn supports_adaptive(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("claude-opus-5")
        || m.starts_with("claude-sonnet-5")
        || m.starts_with("claude-haiku-5")
        || m.starts_with("claude-fable-5")
        || m.starts_with("claude-mythos-5")
        || m.starts_with("claude-opus-4-6")
        || m.starts_with("claude-opus-4-7")
        || m.starts_with("claude-opus-4-8")
        || m.starts_with("claude-sonnet-4-6")
}

/// Models for which the server-side `fallbacks: "default"` form is offered.
fn supports_fallbacks(model: &str) -> bool {
    matches!(model, "claude-fable-5-1" | "claude-opus-5-5" | "claude-opus-5" | "claude-sonnet-5-5")
}

fn valid_effort(e: &str) -> Option<&'static str> {
    Some(match e {
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        "xhigh" => "xhigh",
        "max" => "max",
        _ => return None,
    })
}

// ------------------------------------------------------------ Anthropic

/// Drop blocks that must not be echoed after a mid-output refusal fallback:
/// thinking / redacted_thinking / tool_use / unknown blocks *before* the last
/// `fallback` marker. Text before it and everything after it are kept.
pub fn sanitize_anthropic_content(content: &[Value]) -> Vec<Value> {
    let last_fb = content.iter().rposition(|b| b.get("type").and_then(|t| t.as_str()) == Some("fallback"));
    content
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            let t = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if t == "fallback" {
                return false;
            }
            if t == "text" && b.get("text").and_then(|x| x.as_str()).map(|s| s.is_empty()).unwrap_or(true) {
                return false;
            }
            match last_fb {
                Some(fb) if *i < fb => t == "text",
                _ => true,
            }
        })
        .map(|(_, b)| b.clone())
        .collect()
}

fn anthropic_messages(messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in messages {
        let content: Vec<Value> = if m.role == "assistant" {
            match m.raw.as_ref().and_then(|r| r.as_array()) {
                Some(raw) => sanitize_anthropic_content(raw),
                None => m
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Text { text } if !text.is_empty() => Some(json!({ "type": "text", "text": text })),
                        Block::ToolUse { id, name, input } => Some(json!({ "type": "tool_use", "id": id, "name": name, "input": input })),
                        _ => None,
                    })
                    .collect(),
            }
        } else {
            m.blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } if !text.is_empty() => Some(json!({ "type": "text", "text": text })),
                    Block::ToolResult { tool_use_id, content, is_error } => Some(json!({
                        "type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": is_error
                    })),
                    _ => None,
                })
                .collect()
        };
        if !content.is_empty() {
            out.push(json!({ "role": m.role, "content": content }));
        }
    }
    out
}

pub fn anthropic_body(req: &Request) -> Value {
    let s = req.settings;
    let tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.schema,
                "eager_input_streaming": true,
            })
        })
        .collect();
    let mut body = json!({
        "model": s.model,
        "max_tokens": s.max_tokens.clamp(1024, 128_000),
        "stream": true,
        "cache_control": { "type": "ephemeral" },
        "system": [{ "type": "text", "text": req.system }],
        "messages": anthropic_messages(req.messages),
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if supports_adaptive(&s.model) {
        body["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
        if let Some(e) = valid_effort(&s.effort) {
            body["output_config"] = json!({ "effort": e });
        }
    }
    if supports_fallbacks(&s.model) {
        body["fallbacks"] = json!("default");
    }
    body
}

async fn anthropic_stream(
    req: &Request<'_>,
    on: &mut (dyn FnMut(StreamEvent) + Send),
    cancel: &AtomicBool,
) -> AppResult<Turn> {
    let key = req.api_key.clone().ok_or_else(|| AppError::Invalid("no Anthropic API key is configured".into()))?;
    let body = anthropic_body(req);
    let mut builder = client()?
        .post(ANTHROPIC_URL)
        .header("x-api-key", key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json");
    if body.get("fallbacks").is_some() {
        builder = builder.header("anthropic-beta", FALLBACK_BETA);
    }
    let resp = builder.json(&body).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(api_error("Anthropic", status.as_u16(), &text));
    }

    let mut content: Vec<Value> = Vec::new();
    let mut json_buf: std::collections::HashMap<usize, String> = Default::default();
    let mut turn = Turn {
        blocks: vec![],
        raw: None,
        stop_reason: String::new(),
        stop_detail: None,
        usage: Usage::default(),
        model: req.settings.model.clone(),
        invalid_tool_inputs: vec![],
    };

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let chunk = match tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await {
            Err(_) => return Err(AppError::msg("the AI provider stopped responding")),
            Ok(None) => break,
            Ok(Some(c)) => c?,
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = find_event_end(&buf) {
            let event: String = buf.drain(..pos).collect();
            let data = sse_data(&event);
            if data.is_empty() {
                continue;
            }
            let Ok(ev) = serde_json::from_str::<Value>(&data) else { continue };
            match ev.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "message_start" => {
                    if let Some(m) = ev.pointer("/message/model").and_then(|m| m.as_str()) {
                        turn.model = m.to_string();
                        on(StreamEvent::Model(m.to_string()));
                    }
                    if let Some(u) = ev.pointer("/message/usage") {
                        turn.usage.input_tokens += u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        turn.usage.cache_read_tokens += u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        turn.usage.cache_write_tokens += u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    }
                }
                "content_block_start" => {
                    let idx = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let block = ev.get("content_block").cloned().unwrap_or(Value::Null);
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        on(StreamEvent::ToolStart {
                            id: block.get("id").and_then(|v| v.as_str()).unwrap_or("").into(),
                            name: block.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
                        });
                    }
                    while content.len() <= idx {
                        content.push(Value::Null);
                    }
                    content[idx] = block;
                }
                "content_block_delta" => {
                    let idx = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let Some(block) = content.get_mut(idx) else { continue };
                    let delta = ev.get("delta").cloned().unwrap_or(Value::Null);
                    match delta.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "text_delta" => {
                            let t = delta.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            append_str(block, "text", t);
                            on(StreamEvent::TextDelta(t.to_string()));
                        }
                        "thinking_delta" => {
                            let t = delta.get("thinking").and_then(|v| v.as_str()).unwrap_or("");
                            append_str(block, "thinking", t);
                            on(StreamEvent::ThinkingDelta(t.to_string()));
                        }
                        "signature_delta" => {
                            let s = delta.get("signature").and_then(|v| v.as_str()).unwrap_or("");
                            append_str(block, "signature", s);
                        }
                        "input_json_delta" => {
                            let p = delta.get("partial_json").and_then(|v| v.as_str()).unwrap_or("");
                            json_buf.entry(idx).or_default().push_str(p);
                        }
                        _ => {}
                    }
                }
                "content_block_stop" => {
                    let idx = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    if let (Some(block), Some(raw)) = (content.get_mut(idx), json_buf.remove(&idx)) {
                        // Strict parse — eager streaming means the API did not
                        // validate this input.
                        let trimmed = raw.trim();
                        match if trimmed.is_empty() { Ok(json!({})) } else { serde_json::from_str::<Value>(trimmed) } {
                            Ok(v) if v.is_object() => block["input"] = v,
                            _ => {
                                block["input"] = json!({});
                                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                turn.invalid_tool_inputs.push((id, raw));
                            }
                        }
                    }
                }
                "message_delta" => {
                    if let Some(r) = ev.pointer("/delta/stop_reason").and_then(|v| v.as_str()) {
                        turn.stop_reason = r.to_string();
                    }
                    if let Some(d) = ev.pointer("/delta/stop_details").filter(|v| !v.is_null()) {
                        turn.stop_detail = d
                            .get("explanation")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                            .or_else(|| d.get("category").and_then(|v| v.as_str()).map(String::from));
                    }
                    if let Some(o) = ev.pointer("/usage/output_tokens").and_then(|v| v.as_u64()) {
                        turn.usage.output_tokens = o;
                    }
                }
                "error" => {
                    let msg = ev.pointer("/error/message").and_then(|v| v.as_str()).unwrap_or("stream error");
                    return Err(AppError::msg(format!("Anthropic: {msg}")));
                }
                _ => {}
            }
        }
    }

    content.retain(|b| !b.is_null());
    // Neutral blocks for display and for other providers. Only blocks after
    // the last fallback marker count as executable tool calls.
    let sanitized = sanitize_anthropic_content(&content);
    for b in &content {
        if let Some("thinking") = b.get("type").and_then(|t| t.as_str()) {
            let t = b.get("thinking").and_then(|v| v.as_str()).unwrap_or("");
            if !t.is_empty() {
                turn.blocks.push(Block::Thinking { text: t.to_string() });
            }
        }
    }
    for b in &sanitized {
        match b.get("type").and_then(|t| t.as_str()) {
            Some("text") => turn.blocks.push(Block::Text { text: b["text"].as_str().unwrap_or("").to_string() }),
            Some("tool_use") => turn.blocks.push(Block::ToolUse {
                id: b["id"].as_str().unwrap_or("").to_string(),
                name: b["name"].as_str().unwrap_or("").to_string(),
                input: b.get("input").cloned().unwrap_or(json!({})),
            }),
            _ => {}
        }
    }
    turn.raw = Some(Value::Array(content));
    Ok(turn)
}

fn append_str(block: &mut Value, field: &str, s: &str) {
    let cur = block.get(field).and_then(|v| v.as_str()).unwrap_or("").to_string();
    block[field] = Value::String(cur + s);
}

fn find_event_end(buf: &str) -> Option<usize> {
    let a = buf.find("\n\n").map(|i| i + 2);
    let b = buf.find("\r\n\r\n").map(|i| i + 4);
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    }
}

fn sse_data(event: &str) -> String {
    event
        .lines()
        .filter_map(|l| l.trim_end_matches('\r').strip_prefix("data:"))
        .map(|d| d.strip_prefix(' ').unwrap_or(d))
        .collect::<Vec<_>>()
        .join("\n")
}

fn api_error(provider: &str, status: u16, body: &str) -> AppError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| crate::util::clamp(body.trim(), 300));
    let hint = match status {
        401 | 403 => " — check the API key in Settings",
        404 => " — check the model name",
        429 => " — rate limited; wait a moment and try again",
        529 | 503 => " — the provider is overloaded; try again shortly",
        _ => "",
    };
    AppError::msg(format!("{provider} returned {status}: {detail}{hint}"))
}

// ------------------------------------------------------- OpenAI-compatible

fn openai_messages(system: &str, messages: &[Message]) -> Vec<Value> {
    let mut out = vec![json!({ "role": "system", "content": system })];
    for m in messages {
        if m.role == "assistant" {
            let text: String = m
                .blocks
                .iter()
                .filter_map(|b| if let Block::Text { text } = b { Some(text.as_str()) } else { None })
                .collect::<Vec<_>>()
                .join("");
            let calls: Vec<Value> = m
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::ToolUse { id, name, input } => Some(json!({
                        "id": id, "type": "function",
                        "function": { "name": name, "arguments": input.to_string() }
                    })),
                    _ => None,
                })
                .collect();
            let mut msg = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text) } });
            if !calls.is_empty() {
                msg["tool_calls"] = Value::Array(calls);
            }
            out.push(msg);
        } else {
            for b in &m.blocks {
                match b {
                    Block::Text { text } if !text.is_empty() => out.push(json!({ "role": "user", "content": text })),
                    Block::ToolResult { tool_use_id, content, .. } => {
                        out.push(json!({ "role": "tool", "tool_call_id": tool_use_id, "content": content }))
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

async fn openai_stream(
    req: &Request<'_>,
    on: &mut (dyn FnMut(StreamEvent) + Send),
    cancel: &AtomicBool,
) -> AppResult<Turn> {
    let base = validate_base_url(&req.settings.base_url)?;
    let tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.schema } }))
        .collect();
    let mut body = json!({
        "model": req.settings.model,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": req.settings.max_tokens.clamp(256, 128_000),
        "messages": openai_messages(&req.system, req.messages),
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    let mut builder = client()?.post(format!("{base}/chat/completions")).json(&body);
    if let Some(k) = req.api_key.as_ref().filter(|k| !k.is_empty()) {
        builder = builder.bearer_auth(k);
    }
    let resp = builder.send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(api_error("The AI provider", status.as_u16(), &text));
    }

    let mut text = String::new();
    // index → (id, name, args)
    let mut calls: Vec<(String, String, String)> = Vec::new();
    let mut turn = Turn {
        blocks: vec![],
        raw: None,
        stop_reason: String::new(),
        stop_detail: None,
        usage: Usage::default(),
        model: req.settings.model.clone(),
        invalid_tool_inputs: vec![],
    };
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    'outer: loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let chunk = match tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await {
            Err(_) => return Err(AppError::msg("the AI provider stopped responding")),
            Ok(None) => break,
            Ok(Some(c)) => c?,
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = find_event_end(&buf) {
            let event: String = buf.drain(..pos).collect();
            let data = sse_data(&event);
            if data.trim() == "[DONE]" {
                break 'outer;
            }
            let Ok(ev) = serde_json::from_str::<Value>(&data) else { continue };
            if let Some(m) = ev.get("model").and_then(|m| m.as_str()) {
                turn.model = m.to_string();
            }
            if let Some(u) = ev.get("usage").filter(|u| !u.is_null()) {
                turn.usage.input_tokens = u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                turn.usage.output_tokens = u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            }
            let Some(choice) = ev.pointer("/choices/0") else { continue };
            if let Some(t) = choice.pointer("/delta/content").and_then(|v| v.as_str()) {
                text.push_str(t);
                on(StreamEvent::TextDelta(t.to_string()));
            }
            if let Some(tc) = choice.pointer("/delta/tool_calls").and_then(|v| v.as_array()) {
                for c in tc {
                    let i = c.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    while calls.len() <= i {
                        calls.push(Default::default());
                    }
                    if let Some(id) = c.get("id").and_then(|v| v.as_str()) {
                        calls[i].0 = id.to_string();
                    }
                    if let Some(n) = c.pointer("/function/name").and_then(|v| v.as_str()) {
                        calls[i].1.push_str(n);
                        on(StreamEvent::ToolStart { id: calls[i].0.clone(), name: calls[i].1.clone() });
                    }
                    if let Some(a) = c.pointer("/function/arguments").and_then(|v| v.as_str()) {
                        calls[i].2.push_str(a);
                    }
                }
            }
            if let Some(r) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                turn.stop_reason = match r {
                    "tool_calls" => "tool_use",
                    "length" => "max_tokens",
                    "content_filter" => "refusal",
                    _ => "end_turn",
                }
                .to_string();
            }
        }
    }
    if !text.is_empty() {
        turn.blocks.push(Block::Text { text });
    }
    for (n, (id, name, args)) in calls.into_iter().enumerate() {
        let id = if id.is_empty() { format!("call_{n}") } else { id };
        let input = match serde_json::from_str::<Value>(if args.trim().is_empty() { "{}" } else { &args }) {
            Ok(v) if v.is_object() => v,
            _ => {
                turn.invalid_tool_inputs.push((id.clone(), args));
                json!({})
            }
        };
        turn.blocks.push(Block::ToolUse { id, name, input });
    }
    if turn.stop_reason.is_empty() {
        turn.stop_reason = if turn.blocks.iter().any(|b| matches!(b, Block::ToolUse { .. })) { "tool_use" } else { "end_turn" }.into();
    }
    Ok(turn)
}

// ------------------------------------------------------------ public API

pub async fn stream_turn(
    req: &Request<'_>,
    on: &mut (dyn FnMut(StreamEvent) + Send),
    cancel: &AtomicBool,
) -> AppResult<Turn> {
    match req.settings.provider.as_str() {
        "anthropic" => anthropic_stream(req, on, cancel).await,
        "openai_compatible" => openai_stream(req, on, cancel).await,
        other => Err(AppError::Invalid(format!("unknown AI provider '{other}'"))),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionTest {
    pub ok: bool,
    pub message: String,
    pub models: Vec<String>,
}

/// Validate credentials by listing models (no tokens are spent).
pub async fn test_connection(settings: &AiSettings, key: Option<String>) -> AppResult<ConnectionTest> {
    let c = client()?;
    let resp = match settings.provider.as_str() {
        "anthropic" => {
            let key = key.ok_or_else(|| AppError::Invalid("no Anthropic API key is configured".into()))?;
            c.get(format!("{ANTHROPIC_MODELS_URL}?limit=100"))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .timeout(Duration::from_secs(20))
                .send()
                .await?
        }
        "openai_compatible" => {
            let base = validate_base_url(&settings.base_url)?;
            let mut b = c.get(format!("{base}/models")).timeout(Duration::from_secs(20));
            if let Some(k) = key.filter(|k| !k.is_empty()) {
                b = b.bearer_auth(k);
            }
            b.send().await?
        }
        other => return Err(AppError::Invalid(format!("unknown AI provider '{other}'"))),
    };
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let e = api_error(if settings.provider == "anthropic" { "Anthropic" } else { "The AI provider" }, status.as_u16(), &text);
        return Ok(ConnectionTest { ok: false, message: e.to_string(), models: vec![] });
    }
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let models: Vec<String> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
        .unwrap_or_default();
    let has_model = models.is_empty() || models.iter().any(|m| m == &settings.model);
    Ok(ConnectionTest {
        ok: true,
        message: if has_model {
            format!("Connected. {} model{} available.", models.len(), if models.len() == 1 { "" } else { "s" })
        } else {
            format!("Connected, but '{}' isn't in this account's model list.", settings.model)
        },
        models,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rules() {
        assert!(validate_base_url("https://api.openai.com/v1/").is_ok());
        assert!(validate_base_url("http://localhost:11434/v1").is_ok());
        assert!(validate_base_url("http://example.com/v1").is_err());
        assert!(validate_base_url("file:///c:/x").is_err());
    }

    #[test]
    fn body_uses_current_api_shape() {
        let s = AiSettings::default();
        let msgs = vec![Message::user_text("hi")];
        let tools = vec![ToolDef { name: "t", description: "d", schema: json!({"type":"object","properties":{}}) }];
        let req = Request { settings: &s, api_key: None, system: "sys".into(), tools: &tools, messages: &msgs };
        let b = anthropic_body(&req);
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["thinking"]["type"], "adaptive");
        assert_eq!(b["output_config"]["effort"], "medium");
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["tools"][0]["eager_input_streaming"], true);
        assert!(b.get("temperature").is_none());
        assert!(b["thinking"].get("budget_tokens").is_none());
    }

    #[test]
    fn haiku_4_5_gets_no_adaptive_thinking() {
        let mut s = AiSettings::default();
        s.model = "claude-haiku-4-5".into();
        let req = Request { settings: &s, api_key: None, system: "s".into(), tools: &[], messages: &[] };
        let b = anthropic_body(&req);
        assert!(b.get("thinking").is_none());
        assert!(b.get("fallbacks").is_none());
    }

    #[test]
    fn fallback_sanitising() {
        let content = vec![
            json!({"type":"thinking","thinking":"","signature":"a"}),
            json!({"type":"text","text":"partial"}),
            json!({"type":"tool_use","id":"t1","name":"x","input":{}}),
            json!({"type":"fallback","from":{"model":"a"},"to":{"model":"b"}}),
            json!({"type":"thinking","thinking":"","signature":"b"}),
            json!({"type":"text","text":"rest"}),
        ];
        let s = sanitize_anthropic_content(&content);
        let types: Vec<&str> = s.iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(types, vec!["text", "thinking", "text"]);
    }

    #[test]
    fn sse_parsing() {
        let buf = "event: x\r\ndata: {\"a\":1}\r\n\r\nevent: y\ndata: {\"b\":2}\n\n";
        let end = find_event_end(buf).unwrap();
        assert_eq!(sse_data(&buf[..end]), "{\"a\":1}");
    }
}
