//! Power plans via `powercfg` (allow-listed).

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::security::exec::{self, SafeCommand, SchemeGuid};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PowerScheme {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

/// Find the first GUID-shaped token in a line.
fn find_guid(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .find_map(|tok| SchemeGuid::parse(tok).ok().map(|g| g.as_str().to_string()))
}

pub fn parse_list(output: &str) -> Vec<PowerScheme> {
    output
        .lines()
        .filter_map(|line| {
            let guid = find_guid(line)?;
            let name = match (line.find('('), line.rfind(')')) {
                (Some(a), Some(b)) if b > a => line[a + 1..b].trim().to_string(),
                _ => guid.clone(),
            };
            Some(PowerScheme { guid, name, active: line.trim_end().ends_with('*') })
        })
        .collect()
}

pub fn list() -> AppResult<Vec<PowerScheme>> {
    let out = exec::run(&SafeCommand::PowercfgList)?;
    if !out.success() {
        return Err(AppError::msg(format!("powercfg /list failed: {}", out.stderr.trim())));
    }
    Ok(parse_list(&out.stdout))
}

pub fn active_guid() -> AppResult<String> {
    let out = exec::run(&SafeCommand::PowercfgGetActive)?;
    out.stdout
        .lines()
        .find_map(find_guid)
        .ok_or_else(|| AppError::msg("could not determine the active power plan"))
}

pub fn set_active(guid: &str) -> AppResult<()> {
    let g = SchemeGuid::parse(guid)?;
    let schemes = list()?;
    if !schemes.iter().any(|s| s.guid == g.as_str()) {
        return Err(AppError::NotFound(format!(
            "power plan {} is not installed on this PC",
            g.as_str()
        )));
    }
    let out = exec::run(&SafeCommand::PowercfgSetActive(g.clone()))?;
    if !out.success() {
        return Err(AppError::msg(format!(
            "powercfg /setactive failed: {}",
            if out.stderr.trim().is_empty() { out.stdout.trim() } else { out.stderr.trim() }
        )));
    }
    // Verify.
    let now = active_guid()?;
    if now != g.as_str() {
        return Err(AppError::msg("Windows did not switch to the requested power plan"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_english_and_localised_output() {
        let en = "Existing Power Schemes (* Active)\n-----------------------------------\nPower Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *\nPower Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)\n";
        let s = parse_list(en);
        assert_eq!(s.len(), 2);
        assert!(s[0].active && s[0].name == "Balanced");
        assert!(!s[1].active && s[1].name == "High performance");

        let de = "GUID des Energieschemas: 381b4222-f694-41f0-9685-ff5bb260df2e  (Ausbalanciert) *";
        let s = parse_list(de);
        assert_eq!(s[0].name, "Ausbalanciert");
        assert!(s[0].active);
    }

    #[test]
    fn live_list_has_an_active_plan() {
        let schemes = list().unwrap();
        assert!(!schemes.is_empty());
        let active = active_guid().unwrap();
        assert!(schemes.iter().any(|s| s.guid == active));
    }
}
