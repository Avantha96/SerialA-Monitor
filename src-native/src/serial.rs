//! Native serial management with direct memory channels (zero IPC/base64 overhead).
//!
//! One reader thread per open port. Readers push [`SerialEvent`]s into a channel
//! drained by the UI thread, and wake the UI through egui's repaint request, so
//! the UI is idle when the ports are idle. Modbus transactions run on short-lived
//! worker threads so a slow or absent slave never blocks rendering.

use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};

use crate::logger::Logger;
use crate::modbus::{self, ModbusReply, ModbusRequest};
use crate::stats::{Stats, StatsSnapshot};

const READ_TIMEOUT: Duration = Duration::from_millis(20);
const READ_BUF: usize = 16 * 1024;
const MAX_PENDING: usize = 64 * 1024;
const RECONNECT_POLL: Duration = Duration::from_millis(2000);
const PORT_SCAN_EVERY: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct SerialConfig {
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: String,
    pub mode: String, // "generic" | "modbus"
    /// Assert DTR/RTS after opening. Off by default so ESP32-style boards don't reset.
    pub dtr_rts: bool,
    /// Modbus response timeout in milliseconds.
    pub response_timeout_ms: u64,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".into(),
            mode: "generic".into(),
            dtr_rts: false,
            response_timeout_ms: modbus::RESPONSE_TIMEOUT_MS,
        }
    }
}

impl SerialConfig {
    pub fn is_modbus(&self) -> bool {
        self.mode == "modbus"
    }

    pub fn open(&self) -> Result<Box<dyn SerialPort>, String> {
        let data_bits = match self.data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            8 => DataBits::Eight,
            other => return Err(format!("Unsupported data bits: {other}")),
        };
        let stop_bits = match self.stop_bits {
            1 => StopBits::One,
            2 => StopBits::Two,
            other => return Err(format!("Unsupported stop bits: {other}")),
        };
        let parity = match self.parity.as_str() {
            "none" => Parity::None,
            "even" => Parity::Even,
            "odd" => Parity::Odd,
            "mark" | "space" => {
                return Err(format!("{} parity is not supported by driver", self.parity))
            }
            other => return Err(format!("Unknown parity: {other}")),
        };

        serialport::new(self.path.as_str(), self.baud_rate)
            .data_bits(data_bits)
            .stop_bits(stop_bits)
            .parity(parity)
            .flow_control(FlowControl::None)
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|e| e.to_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PortInfo {
    pub path: String,
    pub manufacturer: String,
}

#[derive(Debug, Clone)]
pub enum SerialEvent {
    /// `lines` are complete lines (the first may complete a previously sent `partial`).
    /// `partial` is the current unterminated tail; it replaces any earlier partial.
    Data {
        id: String,
        lines: Vec<Vec<u8>>,
        partial: Option<Vec<u8>>,
    },
    Error {
        id: String,
        error: String,
    },
    Closed {
        id: String,
        /// True when the user asked for the disconnect.
        deliberate: bool,
    },
    Reconnecting {
        id: String,
    },
    Reconnected {
        id: String,
    },
    ModbusDone {
        id: String,
        txn: ModbusTransaction,
    },
    /// The set of serial ports on the system changed.
    PortsChanged(Vec<PortInfo>),
}

/// Result of one Modbus request/response cycle, including the raw frames for the traffic log.
#[derive(Debug, Clone)]
pub struct ModbusTransaction {
    pub reply: ModbusReply,
    pub tx_frame: Vec<u8>,
    pub rx_frame: Vec<u8>,
    pub rtt_ms: u64,
}

/// Bytes received while a Modbus transaction is waiting for its response.
#[derive(Default)]
pub struct Scratch {
    buf: Mutex<Vec<u8>>,
}

impl Scratch {
    pub fn push(&self, data: &[u8]) {
        let mut b = self.buf.lock();
        if b.len() > 4096 {
            b.clear();
        }
        b.extend_from_slice(data);
    }
    pub fn clear(&self) {
        self.buf.lock().clear();
    }
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.lock().clone()
    }
}

pub struct Session {
    pub epoch: u64,
    pub config: SerialConfig,
    pub writer: Mutex<Box<dyn SerialPort>>,
    pub running: Arc<AtomicBool>,
    pub logging: Arc<AtomicBool>,
    /// Write received data to the log as hex bytes instead of raw text (binary protocols).
    pub log_hex: Arc<AtomicBool>,
    pub stats: Arc<Stats>,
    pub scratch: Arc<Scratch>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Arc<Session>>,
    configs: HashMap<String, SerialConfig>,
    auto_reconnect: HashSet<String>,
    reconnecting: HashSet<String>,
    explicit_disconnect: HashSet<String>,
}

pub struct SerialManager {
    inner: Mutex<Inner>,
    logger: Logger,
    next_epoch: AtomicU64,
    event_tx: Sender<SerialEvent>,
    /// Woken whenever an event is posted, so the UI only repaints when there is something new.
    repaint: Mutex<Option<egui::Context>>,
}

impl SerialManager {
    pub fn new(event_tx: Sender<SerialEvent>) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            logger: Logger::spawn(),
            next_epoch: AtomicU64::new(1),
            event_tx,
            repaint: Mutex::new(None),
        }
    }

    pub fn set_repaint_context(&self, ctx: egui::Context) {
        *self.repaint.lock() = Some(ctx);
    }

    fn post(&self, event: SerialEvent) {
        let _ = self.event_tx.send(event);
        if let Some(ctx) = self.repaint.lock().as_ref() {
            ctx.request_repaint();
        }
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    /// Watch for ports appearing/disappearing (USB adapters plugged in or out).
    /// Enumeration is slow on Windows, so it runs here rather than on the UI thread.
    pub fn spawn_port_watcher(self: &Arc<Self>, mut known: Vec<PortInfo>) {
        let weak = Arc::downgrade(self);
        let _ = std::thread::Builder::new()
            .name("serialx-port-watch".into())
            .spawn(move || loop {
                std::thread::sleep(PORT_SCAN_EVERY);
                let Some(manager) = weak.upgrade() else { return };
                let ports = Self::list_ports();
                if ports != known {
                    known = ports.clone();
                    manager.post(SerialEvent::PortsChanged(ports));
                }
            });
    }

    pub fn list_ports() -> Vec<PortInfo> {
        match serialport::available_ports() {
            Ok(ports) => {
                let mut list: Vec<PortInfo> = ports
                    .into_iter()
                    .map(|p| {
                        let manufacturer = match &p.port_type {
                            serialport::SerialPortType::UsbPort(info) => info
                                .product
                                .clone()
                                .or_else(|| info.manufacturer.clone())
                                .unwrap_or_default(),
                            _ => String::new(),
                        };
                        PortInfo {
                            path: p.port_name,
                            manufacturer,
                        }
                    })
                    .collect();
                // COM3 before COM10.
                list.sort_by_key(|p| {
                    let digits: String = p.path.chars().filter(|c| c.is_ascii_digit()).collect();
                    (
                        p.path.trim_end_matches(|c: char| c.is_ascii_digit()).to_string(),
                        digits.parse::<u32>().unwrap_or(0),
                    )
                });
                list
            }
            Err(_) => Vec::new(),
        }
    }

    pub fn connect(self: &Arc<Self>, session_id: &str, config: SerialConfig) -> Result<bool, String> {
        if self.inner.lock().sessions.contains_key(session_id) {
            self.disconnect(session_id);
        }

        let mut port = config.open()?;

        // ESP32 reset/download-mode safety unless the user opted in.
        let _ = port.write_data_terminal_ready(config.dtr_rts);
        let _ = port.write_request_to_send(config.dtr_rts);

        let writer = port
            .try_clone()
            .map_err(|e| format!("Could not duplicate port handle for writing: {e}"))?;

        let running = Arc::new(AtomicBool::new(true));
        let logging = Arc::new(AtomicBool::new(false));
        let log_hex = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Stats::new(Instant::now()));
        let scratch = Arc::new(Scratch::default());
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);

        let session = Arc::new(Session {
            epoch,
            config: config.clone(),
            writer: Mutex::new(writer),
            running: running.clone(),
            logging: logging.clone(),
            log_hex: log_hex.clone(),
            stats: stats.clone(),
            scratch: scratch.clone(),
        });

        {
            let mut inner = self.inner.lock();
            inner.sessions.insert(session_id.to_string(), session);
            inner.configs.insert(session_id.to_string(), config.clone());
            inner.explicit_disconnect.remove(session_id);
        }

        let manager = Arc::clone(self);
        let sid = session_id.to_string();
        let modbus_mode = config.is_modbus();
        std::thread::Builder::new()
            .name(format!("serialx-rx-{sid}"))
            .spawn(move || {
                reader_loop(
                    manager.clone(),
                    sid.clone(),
                    port,
                    running,
                    logging,
                    log_hex,
                    stats,
                    scratch,
                    modbus_mode,
                );
                manager.on_reader_exit(&sid, epoch);
            })
            .map_err(|e| e.to_string())?;

        Ok(true)
    }

    pub fn disconnect(&self, session_id: &str) {
        let session = {
            let mut inner = self.inner.lock();
            inner.explicit_disconnect.insert(session_id.to_string());
            inner.reconnecting.remove(session_id);
            inner.configs.remove(session_id);
            inner.sessions.remove(session_id)
        };
        if let Some(s) = session {
            s.running.store(false, Ordering::Release);
        }
    }

    /// Forget everything about a session (tab closed).
    pub fn forget(&self, session_id: &str) {
        self.disconnect(session_id);
        self.inner.lock().auto_reconnect.remove(session_id);
    }

    fn on_reader_exit(self: &Arc<Self>, session_id: &str, epoch: u64) {
        let (should_reconnect, config, deliberate) = {
            let mut inner = self.inner.lock();
            if inner.sessions.get(session_id).is_some_and(|s| s.epoch != epoch) {
                return;
            }
            inner.sessions.remove(session_id);
            let deliberate = inner.explicit_disconnect.remove(session_id);
            let wants = inner.auto_reconnect.contains(session_id) && !deliberate;
            let cfg = inner.configs.get(session_id).cloned();
            (wants, cfg, deliberate)
        };

        self.logger.stop(session_id);
        self.post(SerialEvent::Closed {
            id: session_id.to_string(),
            deliberate,
        });

        if let (true, Some(cfg)) = (should_reconnect, config) {
            self.start_reconnect(session_id.to_string(), cfg);
        }
    }

    pub fn set_auto_reconnect(&self, session_id: &str, enabled: bool) {
        let mut inner = self.inner.lock();
        if enabled {
            inner.auto_reconnect.insert(session_id.to_string());
        } else {
            inner.auto_reconnect.remove(session_id);
            inner.reconnecting.remove(session_id);
        }
    }

    fn start_reconnect(self: &Arc<Self>, session_id: String, config: SerialConfig) {
        {
            let mut inner = self.inner.lock();
            if !inner.reconnecting.insert(session_id.clone()) {
                return;
            }
        }
        self.post(SerialEvent::Reconnecting {
            id: session_id.clone(),
        });

        let manager = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("serialx-reconnect-{session_id}"))
            .spawn(move || {
                loop {
                    std::thread::sleep(RECONNECT_POLL);
                    {
                        let inner = manager.inner.lock();
                        let give_up = !inner.auto_reconnect.contains(&session_id)
                            || !inner.reconnecting.contains(&session_id)
                            || inner.sessions.contains_key(&session_id);
                        if give_up {
                            break;
                        }
                    }
                    let present = Self::list_ports().iter().any(|p| p.path == config.path);
                    if !present {
                        continue;
                    }
                    if manager.connect(&session_id, config.clone()).is_ok() {
                        manager.post(SerialEvent::Reconnected {
                            id: session_id.clone(),
                        });
                        break;
                    }
                }
                manager.inner.lock().reconnecting.remove(&session_id);
            })
            .ok();
    }

    pub fn write(&self, session_id: &str, data: &[u8], is_hex: bool) -> Result<(), String> {
        let session = match self.inner.lock().sessions.get(session_id) {
            Some(s) => Arc::clone(s),
            None => return Err("Session not connected".into()),
        };

        let result = {
            let mut w = session.writer.lock();
            match w.write_all(data) {
                Ok(()) => w.flush(),
                Err(e) => Err(e),
            }
        };

        match result {
            Ok(()) => {
                session.stats.record_tx(data.len());
                if session.logging.load(Ordering::Relaxed) {
                    let len = data.len() as u32;
                    self.logger
                        .batch(session_id, log_stamp(), Arc::new(data.to_vec()), vec![len], "TX", is_hex);
                }
                Ok(())
            }
            Err(e) => {
                let err_str = e.to_string();
                self.post(SerialEvent::Error {
                    id: session_id.to_string(),
                    error: err_str.clone(),
                });
                Err(err_str)
            }
        }
    }

    /// Start or stop the disk log for a connected session. Returns the log file path when started.
    pub fn set_logging(&self, session_id: &str, enable: bool, label: &str) -> Result<Option<String>, String> {
        let session = match self.inner.lock().sessions.get(session_id) {
            Some(s) => Arc::clone(s),
            None => return Err("Session is not connected".into()),
        };

        if !enable {
            session.logging.store(false, Ordering::Relaxed);
            self.logger.stop(session_id);
            return Ok(None);
        }
        if session.logging.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let path = self.logger.start(log_dir(), session_id, label)?;
        session.logging.store(true, Ordering::Relaxed);
        Ok(Some(path))
    }

    /// Choose hex (binary protocols) or raw text for logged RX data. Takes effect immediately.
    pub fn set_log_hex(&self, session_id: &str, hex: bool) {
        if let Some(s) = self.inner.lock().sessions.get(session_id) {
            s.log_hex.store(hex, Ordering::Relaxed);
        }
    }

    pub fn stats(&self, session_id: &str) -> Option<StatsSnapshot> {
        self.inner
            .lock()
            .sessions
            .get(session_id)
            .map(|s| s.stats.snapshot(8))
    }

    pub fn shutdown(&self) {
        let sessions: Vec<Arc<Session>> = {
            let mut inner = self.inner.lock();
            inner.auto_reconnect.clear();
            inner.reconnecting.clear();
            inner.sessions.drain().map(|(_, s)| s).collect()
        };
        for s in sessions {
            s.running.store(false, Ordering::Release);
        }
        self.logger.close_all();
    }

    /// Run a Modbus transaction on a worker thread; the result arrives as `SerialEvent::ModbusDone`.
    pub fn start_modbus(self: &Arc<Self>, req: ModbusRequest) {
        let manager = Arc::clone(self);
        let id = req.tab_id.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("serialx-modbus-{id}"))
            .spawn(move || {
                let txn = manager.modbus_request(&req);
                manager.post(SerialEvent::ModbusDone {
                    id: req.tab_id.clone(),
                    txn,
                });
            });
        if let Err(e) = spawned {
            self.post(SerialEvent::ModbusDone {
                id,
                txn: ModbusTransaction {
                    reply: ModbusReply::err(e.to_string()),
                    tx_frame: Vec::new(),
                    rx_frame: Vec::new(),
                    rtt_ms: 0,
                },
            });
        }
    }

    /// Blocking Modbus RTU request/response over the session's open port.
    pub fn modbus_request(&self, req: &ModbusRequest) -> ModbusTransaction {
        let started = Instant::now();
        let fail = |msg: String, tx: Vec<u8>| ModbusTransaction {
            reply: ModbusReply::err(msg),
            tx_frame: tx,
            rx_frame: Vec::new(),
            rtt_ms: started.elapsed().as_millis() as u64,
        };

        let session = match self.inner.lock().sessions.get(&req.tab_id) {
            Some(s) => Arc::clone(s),
            None => return fail("No open port for session".into(), Vec::new()),
        };
        if !session.config.is_modbus() {
            return fail(
                "Session is not in Modbus mode — open tab as Modbus RTU".into(),
                Vec::new(),
            );
        }

        let enc = match modbus::encode(req) {
            Ok(e) => e,
            Err(e) => return fail(e, Vec::new()),
        };

        session.scratch.clear();
        {
            let mut w = session.writer.lock();
            let sent = match w.write_all(&enc.frame) {
                Ok(()) => w.flush(),
                Err(e) => Err(e),
            };
            if let Err(e) = sent {
                return fail(format!("Write failed: {e}"), enc.frame.clone());
            }
        }
        session.stats.record_tx(enc.frame.len());
        let logging = session.logging.load(Ordering::Relaxed);
        if logging {
            let len = enc.frame.len() as u32;
            self.logger
                .batch(&req.tab_id, log_stamp(), Arc::new(enc.frame.clone()), vec![len], "TX", true);
        }

        let timeout = Duration::from_millis(session.config.response_timeout_ms.max(10));
        let deadline = Instant::now() + timeout;
        let rx = loop {
            let buf = session.scratch.snapshot();
            if !buf.is_empty() && modbus::needed(&buf, enc.expect_len) == 0 {
                break Ok(buf);
            }
            if Instant::now() >= deadline {
                break Err(buf);
            }
            std::thread::sleep(Duration::from_millis(1));
        };

        let (reply, rx_frame) = match rx {
            Ok(buf) => (modbus::decode(req, &enc, &buf), buf),
            Err(buf) if buf.is_empty() => (
                ModbusReply::err(format!(
                    "Timed out after {} ms waiting for slave",
                    timeout.as_millis()
                )),
                buf,
            ),
            Err(buf) => (
                ModbusReply::err(format!(
                    "Incomplete response ({} of {} bytes)",
                    buf.len(),
                    enc.expect_len
                )),
                buf,
            ),
        };
        if logging && !rx_frame.is_empty() {
            let len = rx_frame.len() as u32;
            self.logger
                .batch(&req.tab_id, log_stamp(), Arc::new(rx_frame.clone()), vec![len], "RX", true);
        }

        ModbusTransaction {
            reply,
            tx_frame: enc.frame,
            rx_frame,
            rtt_ms: started.elapsed().as_millis() as u64,
        }
    }
}

fn log_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// `Documents/SerialMonitorLogs`, falling back to the working directory.
pub fn log_dir() -> std::path::PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.document_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("SerialMonitorLogs")
}

pub fn split_lines(pending: &mut Vec<u8>, lines: &mut Vec<Vec<u8>>) {
    let mut off = 0usize;
    while let Some(idx) = memchr::memchr(b'\n', &pending[off..]) {
        let nl = off + idx;
        let mut end = nl;
        if end > off && pending[end - 1] == b'\r' {
            end -= 1;
        }
        lines.push(pending[off..end].to_vec());
        off = nl + 1;
    }
    if off > 0 {
        pending.drain(..off);
    }
}

#[allow(clippy::too_many_arguments)]
fn reader_loop(
    manager: Arc<SerialManager>,
    sid: String,
    mut port: Box<dyn SerialPort>,
    running: Arc<AtomicBool>,
    logging: Arc<AtomicBool>,
    log_hex: Arc<AtomicBool>,
    stats: Arc<Stats>,
    scratch: Arc<Scratch>,
    modbus_mode: bool,
) {
    use std::io::Read;

    let mut buf = vec![0u8; READ_BUF];
    let mut pending: Vec<u8> = Vec::with_capacity(READ_BUF);
    let mut lines: Vec<Vec<u8>> = Vec::with_capacity(1024);
    // Length of `pending` last shown to the UI as a provisional tail (0 = not shown).
    let mut partial_shown = 0usize;

    while running.load(Ordering::Acquire) {
        let mut partial = None;
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                let chunk = &buf[..n];
                stats.record_rx(chunk);
                if modbus_mode {
                    scratch.push(chunk);
                } else {
                    pending.extend_from_slice(chunk);
                    let before = lines.len();
                    split_lines(&mut pending, &mut lines);
                    if lines.len() > before {
                        partial_shown = 0;
                    }
                    if pending.len() > MAX_PENDING {
                        lines.push(std::mem::take(&mut pending));
                        partial_shown = 0;
                    }
                }
            }
            Err(ref e) if e.kind() == ErrorKind::TimedOut => {
                // Line went idle: show the unterminated tail (e.g. a "> " prompt) provisionally.
                // It stays in `pending`, so the line is completed in place when '\n' arrives
                // instead of being split at every pause in the sender's output.
                if !pending.is_empty() && pending.len() != partial_shown {
                    partial_shown = pending.len();
                    partial = Some(pending.clone());
                }
            }
            Err(e) => {
                if running.load(Ordering::Acquire) {
                    manager.post(SerialEvent::Error {
                        id: sid.clone(),
                        error: e.to_string(),
                    });
                }
                break;
            }
        }

        if !lines.is_empty() || partial.is_some() {
            if !lines.is_empty() && logging.load(Ordering::Relaxed) {
                let lens: Vec<u32> = lines.iter().map(|l| l.len() as u32).collect();
                let mut blob = Vec::with_capacity(lens.iter().sum::<u32>() as usize);
                for l in &lines {
                    blob.extend_from_slice(l);
                }
                manager
                    .logger()
                    .batch(&sid, log_stamp(), Arc::new(blob), lens, "RX", log_hex.load(Ordering::Relaxed));
            }

            manager.post(SerialEvent::Data {
                id: sid.clone(),
                lines: std::mem::take(&mut lines),
                partial,
            });
        }
    }

    if !pending.is_empty() {
        if logging.load(Ordering::Relaxed) {
            let len = pending.len() as u32;
            manager
                .logger()
                .batch(&sid, log_stamp(), Arc::new(pending.clone()), vec![len], "RX", log_hex.load(Ordering::Relaxed));
        }
        manager.post(SerialEvent::Data {
            id: sid,
            lines: vec![pending],
            partial: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_handles_crlf_and_partial_tail() {
        let mut pending = b"a\r\nbb\nccc".to_vec();
        let mut lines = Vec::new();
        split_lines(&mut pending, &mut lines);
        assert_eq!(lines, vec![b"a".to_vec(), b"bb".to_vec()]);
        assert_eq!(pending, b"ccc".to_vec());
    }

    #[test]
    fn config_deserializes_old_settings_without_new_fields() {
        let cfg: SerialConfig =
            serde_json::from_str(r#"{"path":"COM3","baud_rate":115200}"#).unwrap();
        assert_eq!(cfg.path, "COM3");
        assert_eq!(cfg.data_bits, 8);
        assert_eq!(cfg.response_timeout_ms, modbus::RESPONSE_TIMEOUT_MS);
    }
}
