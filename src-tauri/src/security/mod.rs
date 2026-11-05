//! Security primitives.
//!
//! * [`exec`] — the *only* place NOCTURN spawns external programs. Programs and
//!   arguments are fixed at compile time; dynamic values are restricted to
//!   strictly validated tokens (e.g. a GUID). Nothing — and in particular no
//!   AI output — can reach a shell as free text.
//! * [`secrets`] — API keys stored in Windows Credential Manager, never in the
//!   database, config files or frontend.
//! * [`elevation`] — detecting admin rights and running the narrowly scoped
//!   elevated worker for HKLM changes.

pub mod elevation;
pub mod exec;
pub mod secrets;
