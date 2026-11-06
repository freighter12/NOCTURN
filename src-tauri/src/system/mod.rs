//! System inspection: hardware inventory, live telemetry, processes, network,
//! power, diagnostics and health analysis. Everything here is read-only except
//! `power::set_active` and `processes::end_process`, which are only reachable
//! through journaled / audited command handlers.

pub mod bench;
pub mod crash;
pub mod diagnostics;
pub mod driver_centre;
pub mod dns;
pub mod health;
pub mod history;
pub mod info;
pub mod monitor;
pub mod fileinfo;
pub mod gpu;
pub mod network;
pub mod perf;
pub mod power;
pub mod process_control;
pub mod process_view;
pub mod processes;
pub mod rules;
pub mod scheduler;
pub mod services;
