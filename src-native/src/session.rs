//! Tab session state and UI data models.

use std::collections::VecDeque;
use std::time::Instant;

use chrono::{DateTime, Local};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::modbus::ModbusRequest;
use crate::serial::SerialConfig;

/// Scroll-back per tab. Older lines are dropped (and are still in the disk log if enabled).
pub const MAX_MESSAGES: usize = 50_000;
/// Points kept per plot channel.
pub const MAX_PLOT_POINTS: usize = 5_000;
pub const MAX_PLOT_CHANNELS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabType {
    Generic,
    Modbus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    Ascii,
    Hex,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    None,
    Nl,
    Cr,
    NlCr,
}

impl LineEnding {
    pub fn suffix(&self) -> &'static [u8] {
        match self {
            LineEnding::None => b"",
            LineEnding::Nl => b"\n",
            LineEnding::Cr => b"\r",
            LineEnding::NlCr => b"\r\n",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            LineEnding::None => "None",
            LineEnding::Nl => "\\n (LF)",
            LineEnding::Cr => "\\r (CR)",
            LineEnding::NlCr => "\\r\\n (CRLF)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Rx,
    Tx,
    System,
}

#[derive(Debug, Clone)]
pub struct SerialMessage {
    pub id: u64,
    pub timestamp: DateTime<Local>,
    pub payload: Vec<u8>,
    pub kind: MessageKind,
}

/// How Modbus register values are interpreted for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegFormat {
    U16,
    I16,
    /// 32-bit, high word first (ABCD)
    U32,
    I32,
    F32,
    /// 32-bit float, low word first (CDAB)
    F32Swapped,
}

impl RegFormat {
    pub const ALL: [RegFormat; 6] = [
        RegFormat::U16,
        RegFormat::I16,
        RegFormat::U32,
        RegFormat::I32,
        RegFormat::F32,
        RegFormat::F32Swapped,
    ];
    pub fn label(self) -> &'static str {
        match self {
            RegFormat::U16 => "UINT16",
            RegFormat::I16 => "INT16",
            RegFormat::U32 => "UINT32 (AB CD)",
            RegFormat::I32 => "INT32 (AB CD)",
            RegFormat::F32 => "FLOAT32 (AB CD)",
            RegFormat::F32Swapped => "FLOAT32 (CD AB)",
        }
    }
    pub fn is_32bit(self) -> bool {
        !matches!(self, RegFormat::U16 | RegFormat::I16)
    }

    /// Interpret register `i` (and `i+1` for 32-bit formats) of `regs`.
    /// Returns `None` for the second word of a 32-bit pair or when the pair is incomplete.
    pub fn value(self, regs: &[Option<i64>], i: usize) -> Option<f64> {
        let w = |k: usize| regs.get(k).copied().flatten().map(|v| v as u16);
        match self {
            RegFormat::U16 => w(i).map(|v| v as f64),
            RegFormat::I16 => w(i).map(|v| v as i16 as f64),
            _ => {
                if i % 2 != 0 {
                    return None;
                }
                let (a, b) = (w(i)?, w(i + 1)?);
                let hi_lo = ((a as u32) << 16) | b as u32;
                let lo_hi = ((b as u32) << 16) | a as u32;
                Some(match self {
                    RegFormat::U32 => hi_lo as f64,
                    RegFormat::I32 => hi_lo as i32 as f64,
                    RegFormat::F32 => f32::from_bits(hi_lo) as f64,
                    RegFormat::F32Swapped => f32::from_bits(lo_hi) as f64,
                    _ => unreachable!(),
                })
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModbusRow {
    pub address: u16,
    pub raw: Option<i64>,
    pub status: RowStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStatus {
    Idle,
    Pending,
    Ok,
    Error,
}

/// Everything about a tab that is worth restoring on the next launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TabSettings {
    pub name: String,
    pub tab_type: TabType,
    pub config: SerialConfig,
    pub display_mode: DisplayMode,
    pub auto_reconnect: bool,
    pub line_ending: LineEnding,
    pub input_history: Vec<String>,
    pub interval_delay_ms: u64,
    pub show_delta: bool,
    pub framing_start: String,
    pub framing_end: String,
    pub slave_id: u8,
    pub function_code: u8,
    pub start_address: u16,
    pub quantity: u16,
    pub write_value: String,
    pub poll_interval_ms: u64,
    pub reg_format: RegFormat,
    pub scale: f64,
}

impl Default for TabSettings {
    fn default() -> Self {
        Self {
            name: "Session".into(),
            tab_type: TabType::Generic,
            config: SerialConfig::default(),
            display_mode: DisplayMode::Ascii,
            auto_reconnect: false,
            line_ending: LineEnding::Nl,
            input_history: Vec::new(),
            interval_delay_ms: 1000,
            show_delta: false,
            framing_start: "02".into(),
            framing_end: "03".into(),
            slave_id: 1,
            function_code: 3,
            start_address: 0,
            quantity: 10,
            write_value: "0".into(),
            poll_interval_ms: 1000,
            reg_format: RegFormat::U16,
            scale: 1.0,
        }
    }
}

/// Incrementally maintained list of message ids matching the console filter.
#[derive(Default)]
pub struct FilterCache {
    key: Option<(String, bool)>,
    last_checked: u64,
    pub ids: VecDeque<u64>,
    pub regex_error: Option<String>,
}

/// Incrementally parsed CSV telemetry for the plotter.
#[derive(Default)]
pub struct PlotCache {
    last_checked: u64,
    next_x: f64,
    pub channels: Vec<VecDeque<[f64; 2]>>,
}

pub struct TabSession {
    pub id: String,
    pub name: String,
    pub tab_type: TabType,
    pub config: SerialConfig,
    pub is_connected: bool,
    pub display_mode: DisplayMode,
    pub is_logging: bool,
    pub log_path: Option<String>,
    /// Whether the running log records RX as hex (mirrors the reader thread's flag).
    pub log_hex: bool,
    pub auto_reconnect: bool,
    pub messages: VecDeque<SerialMessage>,
    pub next_msg_id: u64,
    /// Id of the RX message currently showing an unterminated line, if any.
    pub open_partial: Option<u64>,

    // Analysis
    pub filter_text: String,
    pub filter_is_regex: bool,
    pub filter_cache: FilterCache,
    pub plot_cache: PlotCache,
    pub show_stats: bool,
    pub show_plotter: bool,
    pub show_delta: bool,
    pub auto_scroll: bool,
    pub paused: bool,

    // Framing highlights
    pub framing_enabled: bool,
    pub framing_start: String,
    pub framing_end: String,

    // Sending
    pub input_text: String,
    pub line_ending: LineEnding,
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,
    pub interval_active: bool,
    pub interval_text: String,
    pub interval_delay_ms: u64,
    pub last_interval_send: Option<Instant>,
    pub send_error: Option<String>,

    // Modbus state
    pub modbus_req: ModbusRequest,
    pub modbus_rows: Vec<ModbusRow>,
    pub modbus_polling: bool,
    pub modbus_poll_interval_ms: u64,
    pub last_modbus_poll: Option<Instant>,
    pub modbus_last_time: Option<String>,
    pub modbus_error: Option<String>,
    pub modbus_in_flight: bool,
    pub modbus_ok: u64,
    pub modbus_fail: u64,
    pub modbus_last_rtt_ms: Option<u64>,
    pub reg_format: RegFormat,
    pub scale: f64,
}

static NEXT_TAB_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl TabSession {
    pub fn new(index: usize, tab_type: TabType) -> Self {
        let settings = TabSettings {
            name: format!("Session {index}"),
            tab_type,
            ..Default::default()
        };
        Self::from_settings(settings)
    }

    pub fn from_settings(mut s: TabSettings) -> Self {
        let seq = NEXT_TAB_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("tab{seq}");
        s.config.mode = match s.tab_type {
            TabType::Generic => "generic".into(),
            TabType::Modbus => "modbus".into(),
        };

        let mut tab = Self {
            id: id.clone(),
            name: s.name,
            tab_type: s.tab_type,
            config: s.config,
            is_connected: false,
            display_mode: s.display_mode,
            is_logging: false,
            log_path: None,
            log_hex: false,
            auto_reconnect: s.auto_reconnect,
            messages: VecDeque::with_capacity(1024),
            next_msg_id: 1,
            open_partial: None,

            filter_text: String::new(),
            filter_is_regex: false,
            filter_cache: FilterCache::default(),
            plot_cache: PlotCache::default(),
            show_stats: false,
            show_plotter: false,
            show_delta: s.show_delta,
            auto_scroll: true,
            paused: false,

            framing_enabled: false,
            framing_start: s.framing_start,
            framing_end: s.framing_end,

            input_text: String::new(),
            line_ending: s.line_ending,
            input_history: s.input_history,
            history_idx: None,
            interval_active: false,
            interval_text: String::new(),
            interval_delay_ms: s.interval_delay_ms,
            last_interval_send: None,
            send_error: None,

            modbus_req: ModbusRequest {
                tab_id: id,
                slave_id: s.slave_id,
                function_code: s.function_code,
                start_address: s.start_address,
                quantity: s.quantity,
                write_value: s.write_value,
            },
            modbus_rows: Vec::new(),
            modbus_polling: false,
            modbus_poll_interval_ms: s.poll_interval_ms,
            last_modbus_poll: None,
            modbus_last_time: None,
            modbus_error: None,
            modbus_in_flight: false,
            modbus_ok: 0,
            modbus_fail: 0,
            modbus_last_rtt_ms: None,
            reg_format: s.reg_format,
            scale: if s.scale == 0.0 { 1.0 } else { s.scale },
        };
        tab.sync_modbus_rows();
        tab
    }

    pub fn settings(&self) -> TabSettings {
        TabSettings {
            name: self.name.clone(),
            tab_type: self.tab_type,
            config: self.config.clone(),
            display_mode: self.display_mode,
            auto_reconnect: self.auto_reconnect,
            line_ending: self.line_ending,
            input_history: self.input_history.clone(),
            interval_delay_ms: self.interval_delay_ms,
            show_delta: self.show_delta,
            framing_start: self.framing_start.clone(),
            framing_end: self.framing_end.clone(),
            slave_id: self.modbus_req.slave_id,
            function_code: self.modbus_req.function_code,
            start_address: self.modbus_req.start_address,
            quantity: self.modbus_req.quantity,
            write_value: self.modbus_req.write_value.clone(),
            poll_interval_ms: self.modbus_poll_interval_ms,
            reg_format: self.reg_format,
            scale: self.scale,
        }
    }

    pub fn title(&self) -> String {
        if self.config.path.is_empty() {
            self.name.clone()
        } else {
            self.config.path.clone()
        }
    }

    pub fn push(&mut self, payload: Vec<u8>, kind: MessageKind) {
        let msg = SerialMessage {
            id: self.next_msg_id,
            timestamp: Local::now(),
            payload,
            kind,
        };
        self.next_msg_id += 1;
        if self.messages.len() >= MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(msg);
    }

    /// Apply a batch from the reader: complete `lines`, then an optional unterminated tail.
    /// A provisional tail shown earlier is updated in place rather than duplicated.
    pub fn push_rx_batch(&mut self, lines: Vec<Vec<u8>>, partial: Option<Vec<u8>>) {
        if self.paused {
            self.open_partial = None;
            return;
        }
        for line in lines {
            match self.open_partial.take() {
                Some(id) if self.replace_payload(id, line.clone()) => {}
                _ => self.push(line, MessageKind::Rx),
            }
        }
        if let Some(tail) = partial {
            let open = self.open_partial;
            match open {
                Some(id) if self.replace_payload(id, tail.clone()) => {}
                _ => {
                    self.push(tail, MessageKind::Rx);
                    self.open_partial = self.messages.back().map(|m| m.id);
                }
            }
        }
    }

    /// Replace a message's payload, invalidating cache entries derived from it.
    fn replace_payload(&mut self, id: u64, payload: Vec<u8>) -> bool {
        let Some(idx) = self.index_of(id) else { return false };
        self.messages[idx].payload = payload;
        self.filter_cache.ids.retain(|&x| x < id);
        self.filter_cache.last_checked = self.filter_cache.last_checked.min(id - 1);
        true
    }

    pub fn push_system_message(&mut self, text: &str) {
        self.push(text.as_bytes().to_vec(), MessageKind::System);
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.open_partial = None;
        self.filter_cache = FilterCache::default();
        self.plot_cache = PlotCache::default();
    }

    /// Index into `messages` for a message id, if it is still in the buffer.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        let front = self.messages.front()?.id;
        let idx = id.checked_sub(front)? as usize;
        (idx < self.messages.len()).then_some(idx)
    }

    /// Bring the filter cache up to date; only messages not yet checked are scanned.
    pub fn update_filter(&mut self) {
        if self.filter_text.is_empty() {
            self.filter_cache = FilterCache::default();
            return;
        }
        let key = (self.filter_text.clone(), self.filter_is_regex);
        if self.filter_cache.key.as_ref() != Some(&key) {
            self.filter_cache = FilterCache {
                key: Some(key),
                ..Default::default()
            };
        }

        let regex = if self.filter_is_regex {
            match Regex::new(&self.filter_text) {
                Ok(r) => Some(r),
                Err(e) => {
                    self.filter_cache.regex_error = Some(e.to_string());
                    self.filter_cache.ids.clear();
                    return;
                }
            }
        } else {
            None
        };
        self.filter_cache.regex_error = None;
        let needle = self.filter_text.to_lowercase();

        let front = self.messages.front().map(|m| m.id).unwrap_or(u64::MAX);
        while self.filter_cache.ids.front().is_some_and(|&id| id < front) {
            self.filter_cache.ids.pop_front();
        }

        let last = self.filter_cache.last_checked;
        for msg in self.messages.iter().filter(|m| m.id > last) {
            let asc = String::from_utf8_lossy(&msg.payload);
            let hex = hex_string(&msg.payload);
            let matched = match &regex {
                Some(re) => re.is_match(&asc) || re.is_match(&hex),
                None => {
                    asc.to_lowercase().contains(&needle) || hex.to_lowercase().contains(&needle)
                }
            };
            if matched {
                self.filter_cache.ids.push_back(msg.id);
            }
        }
        if let Some(m) = self.messages.back() {
            self.filter_cache.last_checked = m.id;
        }
    }

    /// Parse newly received lines of comma/space/tab-separated numbers into plot channels.
    pub fn update_plot(&mut self) {
        let last = self.plot_cache.last_checked;
        // Never plot a provisional line; wait until it is complete.
        let limit = self.open_partial.unwrap_or(u64::MAX);
        let cache = &mut self.plot_cache;
        let mut checked = last;
        for msg in self.messages.iter().filter(|m| m.id > last && m.id < limit) {
            checked = msg.id;
            if msg.kind != MessageKind::Rx {
                continue;
            }
            let line = String::from_utf8_lossy(&msg.payload);
            let mut values = Vec::new();
            let mut ok = true;
            for part in line
                .split([',', ';', '\t', ' '])
                .filter(|p| !p.is_empty())
            {
                // Accept "label:value" as well as bare numbers.
                let num = part.rsplit(':').next().unwrap_or(part).trim();
                match num.parse::<f64>() {
                    Ok(v) if v.is_finite() => values.push(v),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok || values.is_empty() {
                continue;
            }
            for (ch, v) in values.into_iter().take(MAX_PLOT_CHANNELS).enumerate() {
                if cache.channels.len() <= ch {
                    cache.channels.push(VecDeque::new());
                }
                let series = &mut cache.channels[ch];
                if series.len() >= MAX_PLOT_POINTS {
                    series.pop_front();
                }
                series.push_back([cache.next_x, v]);
            }
            cache.next_x += 1.0;
        }
        self.plot_cache.last_checked = checked;
    }

    /// Treat everything up to `id` as already plotted.
    pub fn plot_cache_skip_to(&mut self, id: u64) {
        self.plot_cache.last_checked = id;
    }

    pub fn sync_modbus_rows(&mut self) {
        let qty = self.modbus_req.quantity as usize;
        let start = self.modbus_req.start_address;
        if self.modbus_rows.len() != qty || self.modbus_rows.first().map(|r| r.address) != Some(start) {
            self.modbus_rows = (0..qty)
                .map(|i| ModbusRow {
                    address: start.wrapping_add(i as u16),
                    raw: None,
                    status: RowStatus::Idle,
                })
                .collect();
        }
    }
}

pub fn hex_string(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

/// Parse "0A FF,10" style input. Whitespace and commas are separators; an optional 0x prefix is allowed.
pub fn parse_hex_bytes(input: &str) -> Result<Vec<u8>, String> {
    let cleaned = input.replace("0x", "").replace("0X", "");
    let mut nibbles: Vec<u8> = Vec::with_capacity(cleaned.len());
    for c in cleaned.chars() {
        if c.is_ascii_whitespace() || c == ',' {
            continue;
        }
        match c.to_digit(16) {
            Some(d) => nibbles.push(d as u8),
            None => return Err(format!("'{c}' is not a hex digit")),
        }
    }
    if nibbles.is_empty() {
        return Err("No hex bytes to send".into());
    }
    if nibbles.len() % 2 != 0 {
        return Err("Hex input needs an even number of digits".into());
    }
    Ok(nibbles.chunks_exact(2).map(|p| (p[0] << 4) | p[1]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex_bytes("0A ff,10").unwrap(), vec![0x0A, 0xFF, 0x10]);
        assert_eq!(parse_hex_bytes("0x01 0x02").unwrap(), vec![1, 2]);
        assert!(parse_hex_bytes("ABC").is_err());
        assert!(parse_hex_bytes("zz").is_err());
        assert!(parse_hex_bytes("  ").is_err());
    }

    #[test]
    fn ring_buffer_and_index_of() {
        let mut t = TabSession::new(1, TabType::Generic);
        for i in 0..(MAX_MESSAGES + 10) {
            t.push(vec![i as u8], MessageKind::Rx);
        }
        assert_eq!(t.messages.len(), MAX_MESSAGES);
        let front = t.messages.front().unwrap().id;
        assert_eq!(front, 11);
        assert_eq!(t.index_of(front), Some(0));
        assert_eq!(t.index_of(front - 1), None);
        assert_eq!(t.index_of(t.next_msg_id), None);
    }

    #[test]
    fn filter_is_incremental() {
        let mut t = TabSession::new(1, TabType::Generic);
        t.push(b"hello".to_vec(), MessageKind::Rx);
        t.push(b"world".to_vec(), MessageKind::Rx);
        t.filter_text = "HEL".into();
        t.update_filter();
        assert_eq!(t.filter_cache.ids.len(), 1);
        t.push(b"help".to_vec(), MessageKind::Rx);
        t.update_filter();
        assert_eq!(t.filter_cache.ids.iter().copied().collect::<Vec<_>>(), vec![1, 3]);

        t.filter_is_regex = true;
        t.filter_text = "^w".into();
        t.update_filter();
        assert_eq!(t.filter_cache.ids.iter().copied().collect::<Vec<_>>(), vec![2]);

        t.filter_text = "(".into();
        t.update_filter();
        assert!(t.filter_cache.regex_error.is_some());
    }

    #[test]
    fn partial_lines_are_completed_in_place() {
        let mut t = TabSession::new(1, TabType::Generic);
        t.filter_text = "received".into();
        // Reader shows "Queue r" provisionally, then more, then the line completes.
        t.push_rx_batch(vec![], Some(b"Queue r".to_vec()));
        t.update_filter();
        t.push_rx_batch(vec![], Some(b"Queue receiv".to_vec()));
        t.push(b"cmd".to_vec(), MessageKind::Tx); // TX in between doesn't break the line
        t.push_rx_batch(vec![b"Queue received: l".to_vec(), b"next".to_vec()], Some(b"> ".to_vec()));
        let texts: Vec<_> = t.messages.iter().map(|m| String::from_utf8_lossy(&m.payload).to_string()).collect();
        assert_eq!(texts, vec!["Queue received: l", "cmd", "next", "> "]);
        assert_eq!(t.open_partial, Some(4));
        t.update_filter();
        assert_eq!(t.filter_cache.ids.iter().copied().collect::<Vec<_>>(), vec![1]);
        // Prompt completes.
        t.push_rx_batch(vec![b"> ok".to_vec()], None);
        assert_eq!(t.messages.len(), 4);
        assert_eq!(t.messages[3].payload, b"> ok");
        assert_eq!(t.open_partial, None);
    }

    #[test]
    fn plot_skips_provisional_line() {
        let mut t = TabSession::new(1, TabType::Generic);
        t.push_rx_batch(vec![], Some(b"1,".to_vec()));
        t.update_plot();
        assert!(t.plot_cache.channels.is_empty());
        t.push_rx_batch(vec![b"1,2".to_vec()], None);
        t.update_plot();
        assert_eq!(t.plot_cache.channels.len(), 2);
    }

    #[test]
    fn plot_parses_csv_and_labels() {
        let mut t = TabSession::new(1, TabType::Generic);
        t.push(b"1.5, 2, -3".to_vec(), MessageKind::Rx);
        t.push(b"temp:20 hum:40".to_vec(), MessageKind::Rx);
        t.push(b"boot ok".to_vec(), MessageKind::Rx);
        t.push(b"9,9".to_vec(), MessageKind::Tx);
        t.update_plot();
        assert_eq!(t.plot_cache.channels.len(), 3);
        assert_eq!(t.plot_cache.channels[0].len(), 2);
        assert_eq!(t.plot_cache.channels[0][1], [1.0, 20.0]);
        assert_eq!(t.plot_cache.channels[2].len(), 1);
    }

    #[test]
    fn register_formats() {
        let regs = vec![Some(0x4148), Some(0x0000), Some(0xFFFF), Some(0xFFFE)];
        assert_eq!(RegFormat::F32.value(&regs, 0), Some(12.5));
        assert_eq!(RegFormat::F32.value(&regs, 1), None);
        assert_eq!(RegFormat::I16.value(&regs, 2), Some(-1.0));
        assert_eq!(RegFormat::I32.value(&regs, 2), Some(-2.0));
        assert_eq!(RegFormat::U32.value(&regs, 2), Some(4294967294.0));
        let swapped = vec![Some(0x0000), Some(0x4148)];
        assert_eq!(RegFormat::F32Swapped.value(&swapped, 0), Some(12.5));
    }
}
