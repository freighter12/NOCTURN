//! Persistent user settings (stored as one JSON document in SQLite).
//! API keys are deliberately *not* here — see `security::secrets`.

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::AppResult;

const KEY: &str = "app_settings";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AiSettings {
    /// `anthropic` or `openai_compatible`.
    pub provider: String,
    pub model: String,
    /// Only used by OpenAI-compatible providers (OpenAI, OpenRouter, LM Studio,
    /// Ollama…). Must be https unless it points at localhost.
    pub base_url: String,
    /// Claude effort level: low | medium | high | xhigh | max.
    pub effort: String,
    pub max_tokens: u32,
    /// Let the assistant read live system information through its tools.
    pub allow_system_context: bool,
    /// Extra instructions appended to the system prompt.
    pub custom_instructions: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
            base_url: "https://api.openai.com/v1".into(),
            effort: "medium".into(),
            max_tokens: 32_000,
            allow_system_context: true,
            custom_instructions: String::new(),
        }
    }
}

/// Hardware monitoring: sampling rate, pause, and alert thresholds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MonitorSettings {
    /// Sampling interval: 1000, 2000 or 5000 ms.
    pub interval_ms: u32,
    /// Stop sampling hardware (process rules keep working at a slow rate).
    pub paused: bool,
    pub alerts: bool,
    pub cpu_alert_pct: f32,
    pub ram_alert_pct: f32,
    pub gpu_temp_alert_c: f32,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        MonitorSettings { interval_ms: 1000, paused: false, alerts: true, cpu_alert_pct: 95.0, ram_alert_pct: 92.0, gpu_temp_alert_c: 85.0 }
    }
}

impl MonitorSettings {
    /// Clamp user input to safe, meaningful values.
    pub fn sanitised(mut self) -> Self {
        self.interval_ms = match self.interval_ms {
            0..=1500 => 1000,
            1501..=3500 => 2000,
            _ => 5000,
        };
        self.cpu_alert_pct = self.cpu_alert_pct.clamp(50.0, 100.0);
        self.ram_alert_pct = self.ram_alert_pct.clamp(50.0, 100.0);
        self.gpu_temp_alert_c = self.gpu_temp_alert_c.clamp(50.0, 110.0);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    /// Pre-tick "Create a restore point" when changes need admin rights.
    pub restore_point_default: bool,
    /// Require an extra confirmation for Advanced-risk changes.
    pub confirm_advanced: bool,
    /// Reduce UI motion.
    pub reduce_motion: bool,
    /// The close button hides NOCTURN to the tray instead of quitting.
    pub close_to_tray: bool,
    /// Windows notifications for maintenance alerts while NOCTURN isn't in front.
    pub desktop_notifications: bool,
    /// Look for a newer version at start-up and daily (never installs by itself).
    pub auto_update_check: bool,
    /// Play songs that aren't in the library from YouTube (unofficial; off by default).
    pub music_streaming: bool,
    pub ai: AiSettings,
    pub monitor: MonitorSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings { restore_point_default: true, confirm_advanced: true, reduce_motion: false, close_to_tray: true, desktop_notifications: true, auto_update_check: true, music_streaming: false, ai: AiSettings::default(), monitor: MonitorSettings::default() }
    }
}

pub fn load(db: &Db) -> AppSettings {
    db.get_setting::<AppSettings>(KEY).ok().flatten().unwrap_or_default()
}

pub fn save(db: &Db, s: &AppSettings) -> AppResult<()> {
    db.set_setting(KEY, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let mut s = load(&db);
        assert_eq!(s, AppSettings::default());
        s.ai.model = "x".into();
        save(&db, &s).unwrap();
        assert_eq!(load(&db).ai.model, "x");
    }

    #[test]
    fn tolerates_partial_documents() {
        let s: AppSettings = serde_json::from_str(r#"{"ai":{"model":"m"}}"#).unwrap();
        assert_eq!(s.ai.model, "m");
        assert_eq!(s.ai.provider, "anthropic");
        assert!(s.restore_point_default);
    }
}
