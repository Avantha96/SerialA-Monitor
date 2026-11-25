//! Traffic statistics and RX byte-frequency histogram.

use egui::{RichText, Ui, Vec2};

use crate::serial::SerialManager;
use crate::session::TabSession;
use crate::theme::palette;

pub fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

pub fn render_stats_panel(tab: &mut TabSession, manager: &SerialManager, ui: &mut Ui) {
    let pal = palette(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new("📊 Statistics").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✖").clicked() {
                tab.show_stats = false;
            }
        });
    });
    ui.separator();

    let Some(s) = manager.stats(&tab.id) else {
        ui.label(RichText::new("Connect to collect statistics.").color(pal.text_muted));
        return;
    };

    egui::Grid::new(("stats_grid", &tab.id))
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("RX total");
            ui.colored_label(pal.rx, fmt_bytes(s.total_rx));
            ui.end_row();
            ui.label("TX total");
            ui.colored_label(pal.tx, fmt_bytes(s.total_tx));
            ui.end_row();
            ui.label("RX rate");
            ui.colored_label(pal.rx, format!("{}/s", fmt_bytes(s.rx_rate)));
            ui.end_row();
            ui.label("TX rate");
            ui.colored_label(pal.tx, format!("{}/s", fmt_bytes(s.tx_rate)));
            ui.end_row();
            ui.label("RX reads");
            ui.label(s.rx_chunks.to_string());
            ui.end_row();
            ui.label("TX writes");
            ui.label(s.tx_chunks.to_string());
            ui.end_row();
            ui.label("Buffered lines");
            ui.label(tab.messages.len().to_string());
            ui.end_row();
        });

    ui.add_space(10.0);
    ui.label(RichText::new("Most frequent RX bytes").strong().size(11.0));
    if s.top_bytes.is_empty() {
        ui.label(RichText::new("No data yet").size(11.0).color(pal.text_muted));
        return;
    }
    let max = s.top_bytes.iter().map(|b| b.count).max().unwrap_or(1) as f32;
    for bc in &s.top_bytes {
        ui.horizontal(|ui| {
            let printable = if bc.byte.is_ascii_graphic() {
                format!("'{}'", bc.byte as char)
            } else {
                "   ".into()
            };
            ui.label(RichText::new(format!("0x{:02X} {printable}", bc.byte)).monospace().size(11.0));
            let width = 90.0 * (bc.count as f32 / max).clamp(0.04, 1.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, pal.ok);
            ui.label(RichText::new(bc.count.to_string()).size(10.0).color(pal.text_dim));
        });
    }
}
