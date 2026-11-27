//! Serial session management: one blocking reader thread per port.
//!
//! Performance notes (this is the hot path):
//!   * Reads use a 16 KiB buffer with a 20 ms timeout. The timeout doubles as
//!     the "line is idle, flush the partial tail" signal, replacing the old
//!     50 ms debounce timer.
//!   * Lines are accumulated into ONE contiguous blob plus a length table and
//!     shipped to the webview as a single base64 event, coalesced to at most
//!     one event per 8 ms. The frontend decodes once and hands out subarray
//!     views, so N lines cost one allocation instead of N JSON number arrays.
//!   * Disk logging and stats never block the read: logging is a channel send
//!     to a dedicated thread, stats are one mutex per chunk.

use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use tauri::{AppHandle, Emitter};

use crate::logger::Logger;
use crate::modbus;
use crate::stats::{Stats, StatsSnapshot};

/// Read timeout, and therefore the worst-case latency before an unterminated
/// tail is flushed to the console.
const READ_TIMEOUT: Duration = Duration::from_millis(20);
/// Minimum spacing between data events; caps IPC traffic under heavy load.
const EMIT_INTERVAL: Duration = Duration::from_millis(8);
/// Emit early once this many lines are queued, whatever the timer says.
const MAX_LINES_PER_EVENT: usize = 1024;
const READ_BUF: usize = 16 * 1024;
/// A stream with no newline at all still has to reach the screen eventually.
const MAX_PENDING: usize = 64 * 1024;
const RECONNECT_POLL: Duration = Duration::from_millis(2000);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialConfig {
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: String,
    /// "generic" | "modbus" - a Modbus session routes bytes to the RTU
    /// scratch buffer instead of the console.
    #[serde(default)]
    pub mode: Option<String>,
}

impl SerialConfig {
    fn is_modbus(&self) -> bool {
        self.mode.as_deref() == Some("modbus")
    }

    fn open(&self) -> Result<Box<dyn SerialPort>, String> {
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
            // The cross-platform driver layer exposes only none/even/odd.
            "mark" | "space" => {
                return Err(format!("{} parity is not supported by the driver", self.parity))
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

#[derive(Serialize, Clone)]
pub struct PortInfo {
    pub path: String,
    pub manufacturer: String,
}

/// Event payload for received data: one blob, one length table.
#[derive(Serialize, Clone)]
struct DataEvent {
    id: String,
    blob: String,
    lens: Vec<u32>,
}

/// Raw receive buffer used only by Modbus sessions.
#[derive(Default)]
pub struct Scratch {
    buf: Mutex<Vec<u8>>,
}

impl Scratch {
    fn push(&self, data: &[u8]) {
        let mut b = self.buf.lock();
        // A runaway slave must not grow this without bound.
        if b.len() > 4096 {
            b.clear();
        }
        b.extend_from_slice(data);
    }
    fn clear(&self) {
        self.buf.lock().clear();
    }
    fn snapshot(&self) -> Vec<u8> {
        self.buf.lock().clone()
    }
}

struct Session {
    /// Distinguishes this port instance from any later one reusing the id.
    epoch: u64,
    config: SerialConfig,
    /// Cloned handle so writes never wait on the reader's blocking read.
    writer: Mutex<Box<dyn SerialPort>>,
    running: Arc<AtomicBool>,
    logging: Arc<AtomicBool>,
    stats: Arc<Stats>,
    scratch: Arc<Scratch>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Arc<Session>>,
    /// Last known config per session, kept for reconnects.
    configs: HashMap<String, SerialConfig>,
    auto_reconnect: HashSet<String>,
    reconnecting: HashSet<String>,
    explicit_disconnect: HashSet<String>,
}

pub struct SerialManager {
    app: AppHandle,
    inner: Mutex<Inner>,
    logger: Logger,
    next_epoch: AtomicU64,
}

impl SerialManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            inner: Mutex::new(Inner::default()),
            logger: Logger::spawn(),
            next_epoch: AtomicU64::new(1),
        }
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn list_ports() -> Vec<PortInfo> {
        match serialport::available_ports() {
            Ok(ports) => ports
                .into_iter()
                .map(|p| {
                    let manufacturer = match &p.port_type {
                        serialport::SerialPortType::UsbPort(info) => {
                            info.manufacturer.clone().unwrap_or_default()
                        }
                        _ => String::new(),
                    };
                    PortInfo { path: p.port_name, manufacturer }
                })
                .collect(),
            Err(e) => {
                eprintln!("port enumeration failed: {e}");
                Vec::new()
            }
        }
    }

    pub fn connect(self: &Arc<Self>, session_id: &str, config: SerialConfig) -> Result<bool, String> {
        // Re-connecting an live session replaces it, as the Node build did.
        if self.inner.lock().sessions.contains_key(session_id) {
            self.disconnect(session_id);
        }

        let mut port = config.open()?;

        // Clear DTR and RTS so boards like the ESP32 do not drop into download
        // mode (or sit in reset) just because we opened the port.
        if let Err(e) = port.write_data_terminal_ready(false) {
            eprintln!("failed to clear DTR: {e}");
        }
        if let Err(e) = port.write_request_to_send(false) {
            eprintln!("failed to clear RTS: {e}");
        }

        let writer = port.try_clone().map_err(|e| {
            format!("could not duplicate port handle for writing: {e}")
        })?;

        let running = Arc::new(AtomicBool::new(true));
        let logging = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Stats::new(Instant::now()));
        let scratch = Arc::new(Scratch::default());
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);

        let session = Arc::new(Session {
            epoch,
            config: config.clone(),
            writer: Mutex::new(writer),
            running: running.clone(),
            logging: logging.clone(),
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
            inner.auto_reconnect_stop(session_id);
            inner.sessions.remove(session_id)
        };
        // Dropping the writer plus clearing `running` lets the reader thread
        // exit within one READ_TIMEOUT.
        if let Some(s) = session {
            s.running.store(false, Ordering::Release);
        }
        self.inner.lock().configs.remove(session_id);
    }

    /// Called by the reader thread as it unwinds.
    fn on_reader_exit(self: &Arc<Self>, session_id: &str, epoch: u64) {
        let (should_reconnect, config) = {
            let mut inner = self.inner.lock();
            // A later connect() may already own this id; if so, this exit
            // belongs to a superseded thread and must not disturb it.
            if inner.sessions.get(session_id).is_some_and(|s| s.epoch != epoch) {
                return;
            }
            inner.sessions.remove(session_id);
            let deliberate = inner.explicit_disconnect.remove(session_id);
            let wants = inner.auto_reconnect.contains(session_id) && !deliberate;
            let cfg = inner.configs.get(session_id).cloned();
            (wants, cfg)
        };

        self.logger.stop(session_id);
        let _ = self.app.emit("serial:closed", session_id);

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
                return; // already polling
            }
        }
        let _ = self.app.emit("serial:reconnecting", &session_id);

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
                    match manager.connect(&session_id, config.clone()) {
                        Ok(_) => {
                            let _ = manager.app.emit("serial:reconnected", &session_id);
                            break;
                        }
                        // Port is enumerated but not yet openable - keep trying.
                        Err(_) => continue,
                    }
                }
                manager.inner.lock().reconnecting.remove(&session_id);
            })
            .ok();
    }
}

impl Inner {
    fn auto_reconnect_stop(&mut self, session_id: &str) {
        self.reconnecting.remove(session_id);
    }
}

fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let mut nibbles: Vec<u8> = Vec::with_capacity(input.len());
    for c in input.chars() {
        if c.is_ascii_whitespace() || c == ',' {
            continue;
        }
        match c.to_digit(16) {
            Some(d) => nibbles.push(d as u8),
            None => return Err(format!("'{c}' is not a hex digit")),
        }
    }
    if nibbles.len() % 2 != 0 {
        return Err("Hex input needs an even number of digits".into());
    }
    Ok(nibbles.chunks_exact(2).map(|p| (p[0] << 4) | p[1]).collect())
}

fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl SerialManager {
    pub fn write(&self, session_id: &str, data: &str, is_hex: bool) {
        let session = match self.inner.lock().sessions.get(session_id) {
            Some(s) => Arc::clone(s),
            None => return,
        };

        let bytes = if is_hex {
            match parse_hex(data) {
                Ok(b) => b,
                Err(e) => {
                    let _ = self.app.emit("serial:error", (session_id, e));
                    return;
                }
            }
        } else {
            data.as_bytes().to_vec()
        };
        if bytes.is_empty() {
            return;
        }

        let result = {
            let mut w = session.writer.lock();
            match w.write_all(&bytes) {
                Ok(()) => w.flush(),
                Err(e) => Err(e),
            }
        };
        match result {
            Ok(()) => {
                session.stats.record_tx(bytes.len());
                if session.logging.load(Ordering::Relaxed) {
                    let len = bytes.len() as u32;
                    self.logger
                        .batch(session_id, now_stamp(), Arc::new(bytes), vec![len], is_hex);
                }
            }
            Err(e) => {
                let _ = self.app.emit("serial:error", (session_id, e.to_string()));
            }
        }
    }

    pub fn toggle_logger(
        &self,
        session_id: &str,
        enable: bool,
        documents: std::path::PathBuf,
    ) -> Result<Option<String>, String> {
        let session = match self.inner.lock().sessions.get(session_id) {
            Some(s) => Arc::clone(s),
            None => return Err("Session is not connected".into()),
        };

        if !enable {
            session.logging.store(false, Ordering::Relaxed);
            self.logger.stop(session_id);
            return Ok(None);
        }
        if session.logging.swap(true, Ordering::Relaxed) {
            return Ok(Some("Already logging".into()));
        }
        match self.logger.start(documents, session_id) {
            Ok(path) => Ok(Some(path)),
            Err(e) => {
                session.logging.store(false, Ordering::Relaxed);
                Err(e)
            }
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

    /// One RTU transaction over the session's already-open handle.
    pub fn modbus_request(&self, req: &modbus::ModbusRequest) -> modbus::ModbusReply {
        // Take a handle and release the registry lock immediately: this call
        // blocks for up to RESPONSE_TIMEOUT_MS waiting on the slave.
        let session = match self.inner.lock().sessions.get(&req.tab_id) {
            Some(s) => Arc::clone(s),
            None => return modbus::ModbusReply::err("No open port for session"),
        };
        if !session.config.is_modbus() {
            return modbus::ModbusReply::err(
                "Session is not in Modbus mode - reconnect this tab as Modbus RTU",
            );
        }

        let enc = match modbus::encode(req) {
            Ok(e) => e,
            Err(e) => return modbus::ModbusReply::err(e),
        };

        session.scratch.clear();
        {
            let mut w = session.writer.lock();
            let sent = match w.write_all(&enc.frame) {
                Ok(()) => w.flush(),
                Err(e) => Err(e),
            };
            if let Err(e) = sent {
                return modbus::ModbusReply::err(format!("Write failed: {e}"));
            }
        }
        session.stats.record_tx(enc.frame.len());

        let deadline = Instant::now() + Duration::from_millis(modbus::RESPONSE_TIMEOUT_MS);
        loop {
            let buf = session.scratch.snapshot();
            if !buf.is_empty() && modbus::needed(&buf, enc.expect_len) == 0 {
                return modbus::decode(req, &enc, &buf);
            }
            if Instant::now() >= deadline {
                return if buf.is_empty() {
                    modbus::ModbusReply::err("Timed out waiting for slave response")
                } else {
                    modbus::ModbusReply::err(format!(
                        "Incomplete response ({} of {} bytes)",
                        buf.len(),
                        enc.expect_len
                    ))
                };
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

// ── Reader thread ──────────────────────────────────────────────────────────

fn push_line(line: &[u8], blob: &mut Vec<u8>, lens: &mut Vec<u32>) {
    blob.extend_from_slice(line);
    lens.push(line.len() as u32);
}

/// Drains every complete `\n`-terminated line out of `pending` into the batch,
/// stripping one preceding `\r`. Empty lines are preserved as empty payloads.
fn split_lines(pending: &mut Vec<u8>, blob: &mut Vec<u8>, lens: &mut Vec<u32>) {
    let mut off = 0usize;
    while let Some(idx) = memchr::memchr(b'\n', &pending[off..]) {
        let nl = off + idx;
        let mut end = nl;
        if end > off && pending[end - 1] == b'\r' {
            end -= 1;
        }
        push_line(&pending[off..end], blob, lens);
        off = nl + 1;
    }
    if off > 0 {
        pending.drain(..off);
    }
}

fn flush_batch(
    manager: &SerialManager,
    sid: &str,
    blob: &mut Vec<u8>,
    lens: &mut Vec<u32>,
    logging: &AtomicBool,
) {
    if logging.load(Ordering::Relaxed) {
        manager.logger().batch(
            sid,
            now_stamp(),
            Arc::new(blob.clone()),
            lens.clone(),
            false,
        );
    }
    let event = DataEvent {
        id: sid.to_string(),
        blob: B64.encode(&blob[..]),
        lens: std::mem::take(lens),
    };
    // `clear` keeps the allocation, so steady-state reading does not allocate.
    blob.clear();
    let _ = manager.app.emit("serial:data", event);
}

#[allow(clippy::too_many_arguments)]
fn reader_loop(
    manager: Arc<SerialManager>,
    sid: String,
    mut port: Box<dyn SerialPort>,
    running: Arc<AtomicBool>,
    logging: Arc<AtomicBool>,
    stats: Arc<Stats>,
    scratch: Arc<Scratch>,
    modbus_mode: bool,
) {
    use std::io::Read;

    let mut buf = vec![0u8; READ_BUF];
    let mut pending: Vec<u8> = Vec::with_capacity(READ_BUF);
    let mut blob: Vec<u8> = Vec::with_capacity(READ_BUF);
    let mut lens: Vec<u32> = Vec::with_capacity(MAX_LINES_PER_EVENT);
    let mut last_emit = Instant::now();

    while running.load(Ordering::Acquire) {
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                let chunk = &buf[..n];
                stats.record_rx(chunk);
                if modbus_mode {
                    scratch.push(chunk);
                } else {
                    pending.extend_from_slice(chunk);
                    split_lines(&mut pending, &mut blob, &mut lens);
                    // Binary stream with no newlines: do not buffer forever.
                    if pending.len() > MAX_PENDING {
                        push_line(&pending, &mut blob, &mut lens);
                        pending.clear();
                    }
                }
            }
            // The read timeout is our "line went idle" tick.
            Err(ref e) if e.kind() == ErrorKind::TimedOut => {
                if !pending.is_empty() {
                    push_line(&pending, &mut blob, &mut lens);
                    pending.clear();
                }
            }
            Err(e) => {
                if running.load(Ordering::Acquire) {
                    let _ = manager.app.emit("serial:error", (&sid, e.to_string()));
                }
                break;
            }
        }

        if !lens.is_empty()
            && (lens.len() >= MAX_LINES_PER_EVENT || last_emit.elapsed() >= EMIT_INTERVAL)
        {
            flush_batch(&manager, &sid, &mut blob, &mut lens, &logging);
            last_emit = Instant::now();
        }
    }

    if !pending.is_empty() {
        push_line(&pending, &mut blob, &mut lens);
    }
    if !lens.is_empty() {
        flush_batch(&manager, &sid, &mut blob, &mut lens, &logging);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(input: &[u8]) -> Vec<Vec<u8>> {
        let mut pending = input.to_vec();
        let mut blob = Vec::new();
        let mut lens = Vec::new();
        split_lines(&mut pending, &mut blob, &mut lens);
        let mut out = Vec::new();
        let mut off = 0usize;
        for l in lens {
            out.push(blob[off..off + l as usize].to_vec());
            off += l as usize;
        }
        out
    }

    #[test]
    fn splits_crlf_and_lf_and_keeps_blank_lines() {
        assert_eq!(
            batch(b"one\r\ntwo\n\nthree"),
            vec![b"one".to_vec(), b"two".to_vec(), Vec::new()]
        );
    }

    #[test]
    fn leaves_partial_tail_pending() {
        let mut pending = b"abc\ndef".to_vec();
        let (mut blob, mut lens) = (Vec::new(), Vec::new());
        split_lines(&mut pending, &mut blob, &mut lens);
        assert_eq!(lens, vec![3]);
        assert_eq!(pending, b"def");
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("0A FF 10").unwrap(), vec![0x0A, 0xFF, 0x10]);
        assert!(parse_hex("0A F").is_err());
        assert!(parse_hex("zz").is_err());
    }
}
