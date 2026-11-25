//! Bottom status bar: connection summary, live throughput, log file.

use egui::{RichText, Ui};

use super::stats_panel::fmt_bytes;
use crate::serial::SerialManager;
use crate::session::TabSession;
use crate::theme::{dot, palette};

pub fn render_status_bar(tab: &TabSession, manager: &SerialManager, ui: &mut Ui) {
    let pal = palette(ui);
    ui.horizontal(|ui| {
        let c = &tab.config;
        let parity = match c.parity.as_str() {
            "even" => 'E',
            "odd" => 'O',
            _ => 'N',
        };
        let line = format!("{} {}{}{}", c.baud_rate, c.data_bits, parity, c.stop_bits);
        if tab.is_connected {
            dot(ui, pal.ok);
            ui.colored_label(pal.ok, "Connected");
            ui.label(RichText::new(format!("{} @ {line}", c.path)).color(pal.text_dim));
        } else {
            dot(ui, pal.text_muted);
            ui.colored_label(pal.text_muted, "Disconnected");
            ui.label(RichText::new(line).color(pal.text_muted));
        }

        if let Some(s) = manager.stats(&tab.id) {
            ui.separator();
            ui.colored_label(pal.rx, format!("RX {} ({}/s)", fmt_bytes(s.total_rx), fmt_bytes(s.rx_rate)));
            ui.colored_label(pal.tx, format!("TX {} ({}/s)", fmt_bytes(s.total_tx), fmt_bytes(s.tx_rate)));
        }
        if tab.paused {
            ui.separator();
            ui.colored_label(pal.warn, "⏸ View paused");
        }

        if let (true, Some(path)) = (tab.is_logging, &tab.log_path) {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let resp = ui
                    .add(egui::Label::new(RichText::new(format!("REC {path}")).color(pal.error).size(11.0)).truncate().sense(egui::Sense::click()))
                    .on_hover_text("Click to copy the log path");
                if resp.clicked() {
                    ui.ctx().copy_text(path.clone());
                }
            });
        }
    });
}
