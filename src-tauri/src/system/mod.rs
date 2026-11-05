//! System inspection: hardware inventory, live telemetry, processes, network,
//! power, diagnostics and health analysis. Everything here is read-only except
//! `power::set_active` and `processes::end_process`, which are only reachable
//! through journaled / audited command handlers.

pub mod diagnostics;
pub mod health;
pub mod info;
pub mod monitor;
pub mod network;
pub mod power;
pub mod processes;
