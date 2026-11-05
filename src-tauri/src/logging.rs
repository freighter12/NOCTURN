//! File + console logging via `tracing`.
//!
//! Logs rotate daily under `%APPDATA%\app.nocturn.desktop\logs`. The returned
//! guard must be kept alive for the lifetime of the process so buffered log
//! lines are flushed on exit.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    if std::fs::create_dir_all(log_dir).is_err() {
        return None;
    }
    let appender = tracing_appender::rolling::daily(log_dir, "nocturn.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_env("NOCTURN_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,nocturn_lib=debug,tao=warn,wry=warn"));

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(writer).with_ansi(false))
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();

    if result.is_err() {
        // A subscriber was already installed (e.g. tests); not fatal.
        return None;
    }
    Some(guard)
}
