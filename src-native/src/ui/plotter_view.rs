//! Real-time plotter for numeric telemetry lines ("1.2, 3.4" or "temp:21 hum:40").

use egui::{Color32, RichText, Ui};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::session::{PlotCache, TabSession, MAX_PLOT_POINTS};
use crate::theme::palette;

const PALETTE: [Color32; 8] = [
    Color32::from_rgb(16, 185, 129),
    Color32::from_rgb(59, 130, 246),
    Color32::from_rgb(245, 158, 11),
    Color32::from_rgb(239, 68, 68),
    Color32::from_rgb(168, 85, 247),
    Color32::from_rgb(236, 72, 153),
    Color32::from_rgb(20, 184, 166),
    Color32::from_rgb(132, 204, 22),
];

pub fn render_plotter_view(tab: &mut TabSession, ui: &mut Ui) {
    tab.update_plot();
    let pal = palette(ui);

    ui.horizontal(|ui| {
        ui.label(RichText::new("📈 Plotter").strong());
        ui.label(
            RichText::new(format!("last {MAX_PLOT_POINTS} samples per channel"))
                .size(11.0)
                .color(pal.text_muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✖").on_hover_text("Close plotter").clicked() {
                tab.show_plotter = false;
            }
            if ui.small_button("Reset").on_hover_text("Clear plotted samples").clicked() {
                let last = tab.messages.back().map(|m| m.id).unwrap_or(0);
                tab.plot_cache = PlotCache::default();
                // Don't re-plot what is already in the buffer.
                tab.plot_cache_skip_to(last);
            }
        });
    });

    if tab.plot_cache.channels.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("Waiting for numeric lines, e.g.  1.25, 4.5, 9.81   or   temp:21.5 hum:40")
                    .color(pal.text_muted),
            );
        });
        return;
    }

    Plot::new(("plot", &tab.id))
        .legend(Legend::default())
        .auto_bounds([true, true].into())
        .show(ui, |plot_ui| {
            for (ch, series) in tab.plot_cache.channels.iter().enumerate() {
                let points: PlotPoints = series.iter().copied().collect();
                plot_ui.line(
                    Line::new(points)
                        .name(format!("CH{}", ch + 1))
                        .color(PALETTE[ch % PALETTE.len()])
                        .width(1.5_f32),
                );
            }
        });
}
