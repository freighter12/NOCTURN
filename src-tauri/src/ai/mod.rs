//! NOCTURN AI — provider-independent assistant with controlled system access.
//!
//! * [`provider`] — streaming clients: Anthropic Messages API (native) and
//!   OpenAI-compatible Chat Completions (OpenAI, OpenRouter, LM Studio, Ollama).
//! * [`tools`] — the assistant's *only* window into the PC: fixed, read-only
//!   tools plus `propose_plan`, which never executes anything — it hands a
//!   validated plan to the UI, where the user approves or rejects it.
//! * [`chat`] — the agent loop and UI event stream.
//! * [`store`] — conversation persistence.
//! * [`prompt`] — the system prompt.

pub mod chat;
pub mod prompt;
pub mod provider;
pub mod store;
pub mod tools;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Provider-neutral content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text { text: String },
    /// Reasoning summary (display only; never replayed from here).
    Thinking { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: String,
    pub blocks: Vec<Block>,
    /// Exact provider content (Anthropic assistant turns) for faithful replay,
    /// including thinking blocks and signatures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    /// UI metadata (usage, plan decisions, errors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl Message {
    pub fn user_text(t: impl Into<String>) -> Self {
        Message { role: "user".into(), blocks: vec![Block::Text { text: t.into() }], raw: None, meta: None }
    }
}

/// Per-process AI runtime state: cancellation flags for in-flight replies.
#[derive(Default)]
pub struct AiRuntime {
    pub cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

pub const SECRET_ANTHROPIC: &str = "anthropic";
pub const SECRET_OPENAI: &str = "openai-compatible";

pub fn secret_name(provider: &str) -> &'static str {
    if provider == "anthropic" {
        SECRET_ANTHROPIC
    } else {
        SECRET_OPENAI
    }
}
