//! Application state, event handling and layout.
//!
//! Data flow: reader threads → `SerialEvent` channel → `drain_events` on the UI
//! thread → per-tab `TabSession`. UI widgets record what the user asked for in
//! an [`Actions`] value; `apply_actions` performs it after the frame is laid out.

use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{CentralPanel, Context, Key, SidePanel, TopBottomPanel};
use serde::{Deserialize, Serialize};

use crate::serial::{PortInfo, SerialEvent, SerialManager};
use crate::session::{
    parse_hex_bytes, DisplayMode, MessageKind, RowStatus, TabSession, TabSettings, TabType,
};
use crate::theme::{self, palette, ThemeChoice};
use crate::ui::config_panel::render_config_panel;
use crate::ui::console_view::{render_console_toolbar, render_console_view};
use crate::ui::interval_sender::render_interval_sender;
use crate::ui::modbus_view::{render_modbus_table, render_modbus_toolbar};
use crate::ui::plotter_view::render_plotter_view;
use crate::ui::stats_panel::render_stats_panel;
use crate::ui::status_bar::render_status_bar;
use crate::ui::tab_bar::render_tab_bar;
use crate::ui::Actions;

const STATE_KEY: &str = "seriala_state_v1";
const DEFAULT_FONT: f32 = 13.0;
/// Upper bound on time between frames when nothing is happening.
const IDLE_WAKE: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct PersistedState {
    tabs: Vec<TabSettings>,
    active: usize,
    font_size: f32,
    theme: ThemeChoice,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            font_size: DEFAULT_FONT,
            theme: ThemeChoice::System,
        }
    }
}

pub struct SerialAApp {
    pub tabs: Vec<TabSession>,
    pub active_tab_id: Option<String>,
    pub font_size: f32,
    pub theme: ThemeChoice,
    pub available_ports: Vec<PortInfo>,
    pub manager: Arc<SerialManager>,
    event_rx: Receiver<SerialEvent>,
}

impl SerialAApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let state: PersistedState = cc
            .storage
            .and_then(|s| eframe::get_value(s, STATE_KEY))
            .unwrap_or_default();

        theme::install(&cc.egui_ctx, state.theme);

        let (tx, rx) = mpsc::channel::<SerialEvent>();
        let manager = Arc::new(SerialManager::new(tx));
        manager.set_repaint_context(cc.egui_ctx.clone());
        let available_ports = SerialManager::list_ports();
        manager.spawn_port_watcher(available_ports.clone());

        let mut tabs: Vec<TabSession> = state.tabs.into_iter().map(TabSession::from_settings).collect();
        if tabs.is_empty() {
            tabs.push(TabSession::new(1, TabType::Generic));
        }
        for tab in &tabs {
            if tab.auto_reconnect {
                manager.set_auto_reconnect(&tab.id, true);
            }
        }
        let active = tabs.get(state.active).or(tabs.first()).map(|t| t.id.clone());

        Self {
            tabs,
            active_tab_id: active,
            font_size: state.font_size.clamp(9.0, 28.0),
            theme: state.theme,
            available_ports,
            manager,
            event_rx: rx,
        }
    }

    fn tab_mut(&mut self, id: &str) -> Option<&mut TabSession> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn add_tab(&mut self, tab_type: TabType) {
        let tab = TabSession::new(self.tabs.len() + 1, tab_type);
        self.active_tab_id = Some(tab.id.clone());
        self.tabs.push(tab);
    }

    pub fn remove_tab(&mut self, id: &str) {
        self.manager.forget(id);
        let pos = self.tabs.iter().position(|t| t.id == id);
        self.tabs.retain(|t| t.id != id);
        if self.tabs.is_empty() {
            self.add_tab(TabType::Generic);
        } else if self.active_tab_id.as_deref() == Some(id) {
            let idx = pos.unwrap_or(0).min(self.tabs.len() - 1);
            self.active_tab_id = Some(self.tabs[idx].id.clone());
        }
    }

    fn cycle_tab(&mut self) {
        let idx = self.active_index();
        let next = (idx + 1) % self.tabs.len();
        self.active_tab_id = Some(self.tabs[next].id.clone());
    }

    fn active_index(&self) -> usize {
        self.tabs
            .iter()
            .position(|t| Some(&t.id) == self.active_tab_id.as_ref())
            .unwrap_or(0)
    }

    pub fn connect_session(&mut self, id: &str) {
        let manager = Arc::clone(&self.manager);
        let Some(tab) = self.tab_mut(id) else { return };
        if tab.config.path.is_empty() {
            tab.push_system_message("Select a serial port first.");
            return;
        }
        match manager.connect(id, tab.config.clone()) {
            Ok(_) => {
                tab.is_connected = true;
                let c = &tab.config;
                let msg = format!(
                    "Connected to {} @ {} baud, {}{}{}",
                    c.path,
                    c.baud_rate,
                    c.data_bits,
                    c.parity.chars().next().unwrap_or('n').to_ascii_uppercase(),
                    c.stop_bits
                );
                tab.push_system_message(&msg);
                if tab.is_logging {
                    Self::start_logging(&manager, tab);
                }
            }
            Err(e) => tab.push_system_message(&format!("Could not open {}: {e}", tab.config.path)),
        }
    }

    pub fn disconnect_session(&mut self, id: &str) {
        self.manager.disconnect(id);
        if let Some(tab) = self.tab_mut(id) {
            if tab.is_connected {
                tab.push_system_message("Disconnected.");
            }
            tab.is_connected = false;
            tab.interval_active = false;
            tab.modbus_polling = false;
        }
    }

    fn start_logging(manager: &SerialManager, tab: &mut TabSession) {
        let hex = tab.display_mode == DisplayMode::Hex;
        match manager.set_logging(&tab.id, true, &tab.config.path) {
            Ok(Some(path)) => {
                manager.set_log_hex(&tab.id, hex);
                tab.log_hex = hex;
                let fmt = if hex { "hex bytes" } else { "text" };
                tab.push_system_message(&format!("Logging RX as {fmt} to {path}"));
                tab.log_path = Some(path);
            }
            Ok(None) => {}
            Err(e) => {
                tab.is_logging = false;
                tab.push_system_message(&format!("Logging failed: {e}"));
            }
        }
    }

    fn set_logging(&mut self, id: &str, enable: bool) {
        let manager = Arc::clone(&self.manager);
        let Some(tab) = self.tab_mut(id) else { return };
        tab.is_logging = enable;
        if !tab.is_connected {
            if enable {
                tab.push_system_message("Logging will start when the port is connected.");
            }
            return;
        }
        if enable {
            Self::start_logging(&manager, tab);
        } else {
            let _ = manager.set_logging(id, false, "");
            if let Some(p) = tab.log_path.take() {
                tab.push_system_message(&format!("Log closed: {p}"));
            }
        }
    }

    pub fn send_data(&mut self, id: &str, raw_text: &str) {
        let manager = Arc::clone(&self.manager);
        let Some(tab) = self.tab_mut(id) else { return };
        let is_hex = tab.display_mode == DisplayMode::Hex;
        let bytes = if is_hex {
            match parse_hex_bytes(raw_text) {
                Ok(b) => b,
                Err(e) => {
                    tab.send_error = Some(e);
                    tab.interval_active = false;
                    return;
                }
            }
        } else {
            let mut b = raw_text.as_bytes().to_vec();
            b.extend_from_slice(tab.line_ending.suffix());
            b
        };
        tab.send_error = None;
        if bytes.is_empty() {
            return;
        }
        match manager.write(id, &bytes, is_hex) {
            Ok(()) => tab.push(bytes, MessageKind::Tx),
            Err(e) => {
                tab.send_error = Some(format!("Write failed: {e}"));
                tab.interval_active = false;
            }
        }
    }

    pub fn run_modbus_request(&mut self, id: &str) {
        let manager = Arc::clone(&self.manager);
        let Some(tab) = self.tab_mut(id) else { return };
        if tab.modbus_in_flight {
            return;
        }
        tab.modbus_in_flight = true;
        for r in &mut tab.modbus_rows {
            r.status = RowStatus::Pending;
        }
        manager.start_modbus(tab.modbus_req.clone());
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                SerialEvent::Data { id, lines, partial } => {
                    if let Some(tab) = self.tab_mut(&id) {
                        tab.push_rx_batch(lines, partial);
                    }
                }
                SerialEvent::Error { id, error } => {
                    if let Some(tab) = self.tab_mut(&id) {
                        tab.push_system_message(&format!("Port error: {error}"));
                    }
                }
                SerialEvent::Closed { id, deliberate } => {
                    if let Some(tab) = self.tab_mut(&id) {
                        if !deliberate && tab.is_connected {
                            tab.push_system_message("Port closed (device removed?).");
                        }
                        tab.is_connected = false;
                        tab.log_path = None;
                        tab.open_partial = None;
                    }
                }
                SerialEvent::Reconnecting { id } => {
                    if let Some(tab) = self.tab_mut(&id) {
                        tab.push_system_message("Connection lost — waiting for the device to return…");
                    }
                }
                SerialEvent::Reconnected { id } => {
                    let manager = Arc::clone(&self.manager);
                    if let Some(tab) = self.tab_mut(&id) {
                        tab.is_connected = true;
                        tab.push_system_message("Reconnected.");
                        if tab.is_logging {
                            Self::start_logging(&manager, tab);
                        }
                    }
                }
                SerialEvent::ModbusDone { id, txn } => {
                    if let Some(tab) = self.tab_mut(&id) {
                        apply_modbus_result(tab, txn);
                    }
                }
                SerialEvent::PortsChanged(ports) => self.available_ports = ports,
            }
        }
    }

    /// Fire due interval sends / Modbus polls. Returns how long until the next one is due.
    fn handle_timers(&mut self) -> Option<Duration> {
        let mut sends = Vec::new();
        let mut polls = Vec::new();
        let mut next_due: Option<Duration> = None;
        let mut consider = |d: Duration| {
            next_due = Some(next_due.map_or(d, |n: Duration| n.min(d)));
        };

        for tab in &mut self.tabs {
            if tab.is_connected && tab.interval_active && !tab.interval_text.is_empty() {
                let period = Duration::from_millis(tab.interval_delay_ms.max(10));
                match tab.last_interval_send {
                    Some(last) if last.elapsed() < period => consider(period - last.elapsed()),
                    _ => {
                        tab.last_interval_send = Some(Instant::now());
                        sends.push((tab.id.clone(), tab.interval_text.clone()));
                        consider(period);
                    }
                }
            }

            if tab.is_connected && tab.tab_type == TabType::Modbus && tab.modbus_polling {
                let period = Duration::from_millis(tab.modbus_poll_interval_ms.max(20));
                match tab.last_modbus_poll {
                    Some(last) if last.elapsed() < period => consider(period - last.elapsed()),
                    _ if tab.modbus_in_flight => {} // the completion event triggers a repaint
                    _ => {
                        tab.last_modbus_poll = Some(Instant::now());
                        polls.push(tab.id.clone());
                        consider(period);
                    }
                }
            }
        }

        for (id, text) in sends {
            self.send_data(&id, &text);
        }
        for id in polls {
            self.run_modbus_request(&id);
        }
        next_due
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let (zoom_in, zoom_out, zoom_reset, new_tab, close_tab, next_tab, clear) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                c && (i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)),
                c && i.key_pressed(Key::Minus),
                c && i.key_pressed(Key::Num0),
                c && i.key_pressed(Key::T),
                c && i.key_pressed(Key::W),
                c && i.key_pressed(Key::Tab),
                c && i.key_pressed(Key::L),
            )
        });
        if zoom_in {
            self.font_size = (self.font_size + 1.0).min(28.0);
        }
        if zoom_out {
            self.font_size = (self.font_size - 1.0).max(9.0);
        }
        if zoom_reset {
            self.font_size = DEFAULT_FONT;
        }
        if new_tab {
            self.add_tab(TabType::Generic);
        }
        if close_tab {
            if let Some(id) = self.active_tab_id.clone() {
                self.remove_tab(&id);
            }
        }
        if next_tab {
            self.cycle_tab();
        }
        if clear {
            let idx = self.active_index();
            self.tabs[idx].clear_messages();
        }
    }

    fn apply_actions(&mut self, ctx: &Context, actions: Actions, active_id: &str) {
        if actions.refresh_ports {
            self.available_ports = SerialManager::list_ports();
        }
        if let Some(enabled) = actions.set_auto_reconnect {
            self.manager.set_auto_reconnect(active_id, enabled);
            if let Some(tab) = self.tab_mut(active_id) {
                tab.auto_reconnect = enabled;
            }
        }
        if let Some(enable) = actions.set_logging {
            self.set_logging(active_id, enable);
        }
        if actions.toggle_connect {
            let connected = self.tab_mut(active_id).is_some_and(|t| t.is_connected);
            if connected {
                self.disconnect_session(active_id);
            } else {
                self.connect_session(active_id);
            }
        }
        if let Some(text) = actions.send {
            self.send_data(active_id, &text);
        }
        if actions.modbus_send {
            self.run_modbus_request(active_id);
        }
        if let Some(choice) = actions.set_theme {
            self.theme = choice;
            theme::set(ctx, choice);
        }
        if let Some(d) = actions.font_delta {
            self.font_size = (self.font_size + d).clamp(9.0, 28.0);
        }
        if let Some(id) = actions.activate_tab {
            self.active_tab_id = Some(id);
        }
        if let Some(t) = actions.add_tab {
            self.add_tab(t);
        }
        if let Some(id) = actions.close_tab {
            self.remove_tab(&id);
        }
    }
}

fn apply_modbus_result(tab: &mut TabSession, txn: crate::serial::ModbusTransaction) {
    tab.modbus_in_flight = false;
    tab.modbus_last_rtt_ms = Some(txn.rtt_ms);
    if !txn.tx_frame.is_empty() {
        tab.push(txn.tx_frame, MessageKind::Tx);
    }
    if !txn.rx_frame.is_empty() {
        tab.push(txn.rx_frame, MessageKind::Rx);
    }

    if let Some(err) = txn.reply.error {
        tab.modbus_fail += 1;
        for r in &mut tab.modbus_rows {
            r.status = RowStatus::Error;
        }
        if tab.modbus_error.as_deref() != Some(err.as_str()) {
            tab.push_system_message(&format!("Modbus: {err}"));
        }
        tab.modbus_error = Some(err);
    } else if let Some(vals) = txn.reply.data {
        tab.modbus_ok += 1;
        tab.modbus_error = None;
        tab.modbus_last_time = Some(chrono::Local::now().format("%H:%M:%S%.3f").to_string());
        for (i, row) in tab.modbus_rows.iter_mut().enumerate() {
            match vals.get(i) {
                Some(&v) => {
                    row.raw = Some(v);
                    row.status = RowStatus::Ok;
                }
                None => row.status = RowStatus::Idle,
            }
        }
    }
}

impl eframe::App for SerialAApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        let next_timer = self.handle_timers();
        self.handle_shortcuts(ctx);

        let mut actions = Actions::default();
        let idx = self.active_index();
        let active_id = self.tabs[idx].id.clone();

        TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            render_tab_bar(&self.tabs, Some(&active_id), self.font_size, self.theme, &mut actions, ui);
        });

        let tab = &mut self.tabs[idx];
        let fill = theme::bar_frame;

        TopBottomPanel::top("config_bar").frame(fill(ctx)).show(ctx, |ui| {
            render_config_panel(tab, &self.available_ports, &mut actions, ui);
        });

        TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            render_status_bar(tab, &self.manager, ui);
        });

        match tab.tab_type {
            TabType::Generic => {
                TopBottomPanel::bottom("send_bar").frame(fill(ctx)).show(ctx, |ui| {
                    render_interval_sender(tab, &mut actions, ui);
                });
                if tab.show_stats {
                    SidePanel::right("stats")
                        .resizable(true)
                        .default_width(250.0)
                        .show(ctx, |ui| render_stats_panel(tab, &self.manager, ui));
                }
                if tab.show_plotter {
                    TopBottomPanel::bottom("plotter")
                        .resizable(true)
                        .default_height(240.0)
                        .min_height(120.0)
                        .show(ctx, |ui| render_plotter_view(tab, ui));
                }
                CentralPanel::default().show(ctx, |ui| {
                    render_console_toolbar(tab, ui);
                    ui.separator();
                    render_console_view(tab, self.font_size, ui);
                });
            }
            TabType::Modbus => {
                TopBottomPanel::top("modbus_bar").frame(fill(ctx)).show(ctx, |ui| {
                    render_modbus_toolbar(tab, &mut actions, ui);
                });
                TopBottomPanel::bottom("modbus_traffic")
                    .resizable(true)
                    .default_height(200.0)
                    .min_height(80.0)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Bus traffic").strong());
                            ui.label(
                                egui::RichText::new("raw RTU frames (TX request, RX response)")
                                    .size(11.0)
                                    .color(palette(ui).text_muted),
                            );
                        });
                        render_console_toolbar(tab, ui);
                        render_console_view(tab, self.font_size, ui);
                    });
                CentralPanel::default().show(ctx, |ui| render_modbus_table(tab, ui));
            }
        }

        self.apply_actions(ctx, actions, &active_id);

        // The log format follows the view: HEX view logs RX as hex bytes.
        let manager = Arc::clone(&self.manager);
        if let Some(tab) = self.tab_mut(&active_id) {
            let hex = tab.display_mode == DisplayMode::Hex;
            if tab.is_logging && tab.is_connected && tab.log_hex != hex {
                manager.set_log_hex(&tab.id, hex);
                tab.log_hex = hex;
                let fmt = if hex { "hex bytes" } else { "text" };
                tab.push_system_message(&format!("Log now records RX as {fmt}."));
            }
        }

        // Event-driven repaint: serial threads wake us on data; otherwise only timers do.
        let mut wake = IDLE_WAKE;
        if let Some(d) = next_timer {
            wake = wake.min(d);
        }
        if self.tabs.iter().any(|t| t.is_connected) {
            wake = wake.min(Duration::from_millis(500)); // live stats / status bar
        }
        ctx.request_repaint_after(wake);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = PersistedState {
            tabs: self.tabs.iter().map(|t| t.settings()).collect(),
            active: self.active_index(),
            font_size: self.font_size,
            theme: self.theme,
        };
        eframe::set_value(storage, STATE_KEY, &state);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.manager.shutdown();
    }
}
