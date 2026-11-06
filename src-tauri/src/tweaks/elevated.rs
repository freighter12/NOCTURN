//! Request handler executed inside the elevated (UAC) worker process.
//!
//! Trust model: the request file is treated as untrusted input.
//! * A registry item must name a catalogue tweak and an operation that is
//!   *exactly* one of that tweak's HKLM operations.
//! * A startup item must use the pseudo-ID `startup`, a machine-wide location,
//!   and name an entry that actually exists there.
//! * A DNS item must use the pseudo-ID `dns`, name an existing interface, and
//!   (when applying) set exactly one of the fixed provider lists. Rollback may
//!   restore any list of plain IP addresses of the right family.
//! * Rollback items may only restore a value at a validated target.
//! Anything else is rejected without touching the system.

use serde::Deserialize;
use serde_json::{json, Value};

use super::engine::{apply_local, RestorePointResult};
use super::executor::{capture_original, restore_local};
use super::{catalog, ops_for, ChangeOp, Hive, Original};
use crate::error::{AppError, AppResult};
use crate::security::exec::{self, PsScript};
use crate::system::dns;

#[derive(Debug, Deserialize)]
struct Item {
    change_id: i64,
    tweak_id: String,
    op: ChangeOp,
    /// Rollback only: the state to restore.
    #[serde(default)]
    original: Option<Original>,
}

#[derive(Debug, Deserialize)]
struct Request {
    kind: String,
    #[serde(default)]
    create_restore_point: bool,
    #[serde(default)]
    items: Vec<Item>,
}

/// Validate that `op` may run elevated on behalf of `tweak_id`.
fn validate(tweak_id: &str, op: &ChangeOp, applying: bool) -> AppResult<()> {
    match op {
        ChangeOp::Registry(s) => {
            let tweak = catalog::get(tweak_id)
                .ok_or_else(|| AppError::Forbidden(format!("unknown tweak '{tweak_id}'")))?;
            if s.hive != Hive::Hklm {
                return Err(AppError::Forbidden("HKCU changes are never made by the elevated worker".into()));
            }
            if !ops_for(tweak).iter().any(|o| o == op) {
                return Err(AppError::Forbidden(format!("operation is not part of '{tweak_id}'")));
            }
            Ok(())
        }
        ChangeOp::StartupApproval { location, name, .. } => {
            if tweak_id != "startup" {
                return Err(AppError::Forbidden("startup changes must use the startup ID".into()));
            }
            if !location.is_machine() {
                return Err(AppError::Forbidden("per-user startup entries are never changed elevated".into()));
            }
            if name.is_empty() || name.len() > 260 || name.contains('\0') {
                return Err(AppError::Forbidden("invalid startup entry name".into()));
            }
            if !crate::startup::entry_exists(*location, name) {
                return Err(AppError::Forbidden(format!("startup entry '{name}' does not exist")));
            }
            Ok(())
        }
        ChangeOp::DnsServers { interface, ipv6, servers, .. } => {
            if tweak_id != "dns" {
                return Err(AppError::Forbidden("DNS changes must use the dns ID".into()));
            }
            // Fails for anything that isn't a real interface GUID.
            dns::read(interface, *ipv6)?;
            if applying && dns::identify(servers, *ipv6).is_none() {
                return Err(AppError::Forbidden("only the listed DNS providers can be set".into()));
            }
            dns::validate_servers(servers, *ipv6)
        }
        _ => Err(AppError::Forbidden("this operation never runs elevated".into())),
    }
}

#[cfg(test)]
pub(crate) fn validate_for_test(tweak_id: &str, op: &ChangeOp) -> AppResult<()> {
    validate(tweak_id, op, true)
}

/// The original state must match the op's kind before it's restored.
fn original_fits(op: &ChangeOp, o: &Original) -> bool {
    matches!(
        (op, o),
        (ChangeOp::Registry(_) | ChangeOp::RegistryDelete { .. } | ChangeOp::StartupApproval { .. }, Original::Registry { .. }) | (ChangeOp::DnsServers { .. }, Original::Dns { .. })
    )
}

pub fn create_restore_point() -> RestorePointResult {
    match exec::powershell_json(PsScript::CreateRestorePoint) {
        Ok(v) => RestorePointResult {
            ok: v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false),
            message: v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("no response from System Restore")
                .to_string(),
        },
        Err(e) => RestorePointResult { ok: false, message: e.to_string() },
    }
}

pub fn handle(raw: Value) -> Value {
    let req: Request = match serde_json::from_value(raw) {
        Ok(r) => r,
        Err(e) => return json!({ "error": format!("malformed request: {e}") }),
    };
    if req.kind != "apply" && req.kind != "rollback" {
        return json!({ "error": format!("unknown request kind '{}'", req.kind) });
    }

    let restore_point = if req.kind == "apply" && req.create_restore_point {
        let r = create_restore_point();
        Some(json!({ "ok": r.ok, "message": r.message }))
    } else {
        None
    };

    let mut results = Vec::new();
    for item in &req.items {
        let outcome: AppResult<(Option<Original>, String)> =
            validate(&item.tweak_id, &item.op, req.kind == "apply").and_then(|_| {
                if req.kind == "apply" {
                    let original = capture_original(&item.op)?;
                    let after = apply_local(&item.op)?;
                    Ok((Some(original), after))
                } else {
                    let original = item
                        .original
                        .clone()
                        .filter(|o| original_fits(&item.op, o))
                        .ok_or_else(|| AppError::Forbidden("missing or mismatched original".into()))?;
                    let shown = restore_local(&item.op, &original)?;
                    Ok((None, shown))
                }
            });
        results.push(match outcome {
            Ok((original, after)) => json!({
                "change_id": item.change_id, "ok": true, "original": original, "after": after,
            }),
            Err(e) => json!({ "change_id": item.change_id, "ok": false, "error": e.to_string() }),
        });
    }

    json!({ "restore_point": restore_point, "results": results })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweaks::{RegData, RegSetting, StartupLocation};

    #[test]
    fn rejects_ops_outside_the_catalogue() {
        let evil = ChangeOp::Registry(RegSetting {
            hive: Hive::Hklm,
            path: r"SOFTWARE\Policies\Microsoft\Windows Defender".into(),
            name: "DisableAntiSpyware".into(),
            data: RegData::Dword(1),
        });
        assert!(validate("gaming.hardware_gpu_scheduling", &evil, true).is_err());
        assert!(validate("not.a.tweak", &evil, true).is_err());
    }

    #[test]
    fn rejects_hkcu_and_non_registry_ops() {
        let t = catalog::get("privacy.advertising_id_off").unwrap();
        let op = ops_for(t).remove(0);
        assert!(validate(t.id, &op, true).is_err());
        assert!(validate("network.flush_dns", &ChangeOp::FlushDns, true).is_err());
        assert!(validate("performance.clean_temp", &ChangeOp::CleanTempFiles, true).is_err());
    }

    #[test]
    fn accepts_declared_hklm_op() {
        let t = catalog::get("gaming.hardware_gpu_scheduling").unwrap();
        let op = ops_for(t).remove(0);
        assert!(validate(t.id, &op, true).is_ok());
    }

    #[test]
    fn tampered_value_is_rejected() {
        let t = catalog::get("gaming.hardware_gpu_scheduling").unwrap();
        let mut op = ops_for(t).remove(0);
        if let ChangeOp::Registry(s) = &mut op {
            s.data = RegData::Dword(1);
        }
        assert!(validate(t.id, &op, true).is_err());
    }

    #[test]
    fn startup_validation() {
        let user = ChangeOp::StartupApproval {
            location: StartupLocation::RunUser,
            name: "x".into(),
            enabled: false,
        };
        assert!(validate("startup", &user, true).is_err());
        let missing = ChangeOp::StartupApproval {
            location: StartupLocation::RunMachine,
            name: "NOCTURN-definitely-not-a-real-entry".into(),
            enabled: false,
        };
        assert!(validate("startup", &missing, true).is_err());
        assert!(validate("gaming.game_mode", &missing, true).is_err());
    }

    #[test]
    fn dns_validation() {
        let lo = dns::interface_guid(1).unwrap();
        let op = |servers: &[&str], ipv6: bool| ChangeOp::DnsServers {
            interface: lo.clone(),
            alias: "x".into(),
            ipv6,
            servers: servers.iter().map(|s| s.to_string()).collect(),
        };
        assert!(validate("dns", &op(&["1.1.1.1", "1.0.0.1"], false), true).is_ok());
        assert!(validate("dns", &op(&[], false), true).is_ok(), "automatic");
        assert!(validate("dns", &op(&["2620:fe::fe", "2620:fe::9"], true), true).is_ok());
        // A hijacking resolver can't be applied, only restored as a recorded original.
        assert!(validate("dns", &op(&["203.0.113.9"], false), true).is_err());
        assert!(validate("dns", &op(&["203.0.113.9"], false), false).is_ok());
        assert!(validate("dns", &op(&["not an ip"], false), false).is_err());
        assert!(validate("dns", &op(&["1.1.1.1", "1.0.0.1"], true), true).is_err(), "family mismatch");
        assert!(validate("startup", &op(&["1.1.1.1", "1.0.0.1"], false), true).is_err());
        let bad_if = ChangeOp::DnsServers { interface: "00000000-0000-0000-0000-000000000000".into(), alias: "x".into(), ipv6: false, servers: vec![] };
        assert!(validate("dns", &bad_if, true).is_err());
        // Rollback needs an original of the matching kind.
        let resp = handle(json!({
            "kind": "rollback",
            "items": [{ "change_id": 1, "tweak_id": "dns", "op": op(&[], false), "original": { "type": "power_plan", "scheme": "x" } }]
        }));
        assert_eq!(resp["results"][0]["ok"], json!(false));
    }

    #[test]
    fn handle_reports_validation_errors_without_side_effects() {
        let resp = handle(json!({
            "kind": "apply",
            "items": [{ "change_id": 1, "tweak_id": "x", "op": { "type": "flush_dns" } }]
        }));
        assert_eq!(resp["results"][0]["ok"], json!(false));
        let resp = handle(json!({ "kind": "format_disk" }));
        assert!(resp.get("error").is_some());
    }
}
