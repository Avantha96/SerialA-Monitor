//! SerialA Monitor — native multi-session serial + Modbus RTU monitor.
//!
//! Layering (lower layers never depend on higher ones):
//! - `modbus`, `adl400`      protocol logic, pure and unit-tested
//! - `stats`, `logger`       per-session counters, background disk logging
//! - `serial`                port I/O threads, reconnect, Modbus transactions
//! - `session`               per-tab UI state (buffers, caches, settings)
//! - `theme`, `ui`, `app`    egui presentation

pub mod adl400;
pub mod app;
pub mod logger;
pub mod modbus;
pub mod serial;
pub mod session;
pub mod stats;
pub mod theme;
pub mod ui;
