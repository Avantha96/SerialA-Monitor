//! Virtualized console: only visible rows are laid out, each as a single text job.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, RichText, ScrollArea, TextWrapMode, Ui, Vec2};

use crate::session::{hex_string, DisplayMode, MessageKind, SerialMessage, TabSession, TabType};
use crate::theme::{palette, Palette};

pub fn render_console_toolbar(tab: &mut TabSession, ui: &mut Ui) {
    let pal = palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);

        ui.label("🔍");
        let filter = ui.add(
            egui::TextEdit::singleline(&mut tab.filter_text)
                .hint_text(if tab.filter_is_regex { "Regex…" } else { "Filter…" })
                .desired_width(180.0),
        );
        if let Some(err) = &tab.filter_cache.regex_error {
            filter.on_hover_text(err.as_str());
            ui.colored_label(pal.error, "invalid regex");
        }
        if !tab.filter_text.is_empty() && ui.small_button("✖").on_hover_text("Clear filter").clicked() {
            tab.filter_text.clear();
        }
        ui.toggle_value(&mut tab.filter_is_regex, ".*").on_hover_text("Regular expression");

        ui.separator();
        ui.toggle_value(&mut tab.show_delta, "+Δt").on_hover_text("Time since previous shown line");

        if tab.tab_type == TabType::Generic {
            ui.toggle_value(&mut tab.framing_enabled, "Framing")
                .on_hover_text("Highlight start/end bytes in HEX view");
            if tab.framing_enabled {
                ui.add(egui::TextEdit::singleline(&mut tab.framing_start).desired_width(22.0))
                    .on_hover_text("Start byte (hex)");
                ui.add(egui::TextEdit::singleline(&mut tab.framing_end).desired_width(22.0))
                    .on_hover_text("End byte (hex)");
            }
            ui.separator();
            ui.toggle_value(&mut tab.show_plotter, "📈 Plot");
            ui.toggle_value(&mut tab.show_stats, "📊 Stats");
        }

        ui.separator();
        ui.toggle_value(&mut tab.paused, "⏸ Pause")
            .on_hover_text("Stop adding received lines to the view (disk logging continues)");
        ui.toggle_value(&mut tab.auto_scroll, "⬇ Follow");

        if ui.button("📋 Copy").on_hover_text("Copy shown lines to the clipboard").clicked() {
            let text = visible_text(tab);
            ui.ctx().copy_text(text);
        }
        if ui.button("🗑 Clear").on_hover_text("Clear buffer (Ctrl+L)").clicked() {
            tab.clear_messages();
        }

        let shown = if tab.filter_text.is_empty() {
            tab.messages.len()
        } else {
            tab.filter_cache.ids.len()
        };
        let counter = if tab.filter_text.is_empty() {
            format!("{shown} lines")
        } else {
            format!("{shown} / {} lines", tab.messages.len())
        };
        ui.label(RichText::new(counter).size(11.0).color(pal.text_muted));
    });
}

pub fn render_console_view(tab: &mut TabSession, font_size: f32, ui: &mut Ui) {
    tab.update_filter();
    let pal = palette(ui);
    let font = FontId::monospace(font_size);
    let row_height = ui.fonts(|f| f.row_height(&font)) + 2.0;

    let filtering = !tab.filter_text.is_empty();
    let total = if filtering { tab.filter_cache.ids.len() } else { tab.messages.len() };

    let framing = (tab.framing_enabled && tab.display_mode != DisplayMode::Ascii).then(|| {
        (
            u8::from_str_radix(tab.framing_start.trim(), 16).ok(),
            u8::from_str_radix(tab.framing_end.trim(), 16).ok(),
        )
    });
    // Modbus tabs carry binary frames: always show them as hex.
    let mode = if tab.tab_type == TabType::Modbus { DisplayMode::Hex } else { tab.display_mode };

    if total == 0 {
        ui.centered_and_justified(|ui| {
            let hint = if filtering {
                "No lines match the filter."
            } else if tab.is_connected {
                "Waiting for data…"
            } else {
                "Select a port and press Connect."
            };
            ui.label(RichText::new(hint).color(pal.text_muted));
        });
        return;
    }

    ScrollArea::both()
        .id_salt(("console", &tab.id))
        .auto_shrink([false, false])
        .stick_to_bottom(tab.auto_scroll)
        .show_rows(ui, row_height, total, |ui, range| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let mut prev_time = None;
            if tab.show_delta && range.start > 0 {
                prev_time = message_at(tab, filtering, range.start - 1).map(|m| m.timestamp);
            }
            for row in range {
                let Some(msg) = message_at(tab, filtering, row) else { continue };
                let delta = if tab.show_delta {
                    prev_time.map(|p| (msg.timestamp - p).num_milliseconds().max(0))
                } else {
                    None
                };
                prev_time = Some(msg.timestamp);

                let job = row_job(msg, mode, tab.show_delta, delta, framing, &font, &pal);
                ui.add(egui::Label::new(job).wrap_mode(TextWrapMode::Extend));
            }
        });
}

fn message_at(tab: &TabSession, filtering: bool, row: usize) -> Option<&SerialMessage> {
    if filtering {
        let id = *tab.filter_cache.ids.get(row)?;
        tab.messages.get(tab.index_of(id)?)
    } else {
        tab.messages.get(row)
    }
}

/// Printable text for a payload: control characters become a middle dot.
fn printable(payload: &[u8]) -> String {
    String::from_utf8_lossy(payload)
        .chars()
        .map(|c| if c.is_control() && c != '\t' { '·' } else { c })
        .collect()
}

fn row_job(
    msg: &SerialMessage,
    mode: DisplayMode,
    show_delta: bool,
    delta: Option<i64>,
    framing: Option<(Option<u8>, Option<u8>)>,
    font: &FontId,
    pal: &Palette,
) -> LayoutJob {
    let fmt = |color: Color32| TextFormat::simple(font.clone(), color);
    let mut job = LayoutJob::default();

    job.append(&msg.timestamp.format("%H:%M:%S%.3f ").to_string(), 0.0, fmt(pal.text_muted));
    if show_delta {
        let d = match delta {
            Some(d) => format!("+{d:>5}ms "),
            None => "         ".to_string(),
        };
        job.append(&d, 0.0, fmt(pal.warn));
    }

    let (tag, tag_color) = match msg.kind {
        MessageKind::Rx => ("RX ", pal.rx),
        MessageKind::Tx => ("TX ", pal.tx),
        MessageKind::System => ("-- ", pal.sys),
    };
    job.append(tag, 0.0, fmt(tag_color));

    if msg.kind == MessageKind::System {
        job.append(&String::from_utf8_lossy(&msg.payload), 0.0, fmt(pal.sys));
        return job;
    }

    let text_color = if msg.kind == MessageKind::Tx { pal.tx } else { pal.text };
    match mode {
        DisplayMode::Ascii => job.append(&printable(&msg.payload), 0.0, fmt(text_color)),
        DisplayMode::Hex | DisplayMode::Both => {
            if mode == DisplayMode::Both {
                job.append(&printable(&msg.payload), 0.0, fmt(text_color));
                job.append("  |  ", 0.0, fmt(pal.text_muted));
            }
            match framing {
                Some((start, end)) if msg.kind == MessageKind::Rx => {
                    for (i, &b) in msg.payload.iter().enumerate() {
                        let mut f = fmt(pal.hex);
                        if Some(b) == start {
                            f.color = pal.ok;
                            f.background = pal.ok.gamma_multiply(0.25);
                        } else if Some(b) == end {
                            f.color = pal.error;
                            f.background = pal.error.gamma_multiply(0.25);
                        }
                        if i > 0 {
                            job.append(" ", 0.0, fmt(pal.hex));
                        }
                        job.append(&format!("{b:02X}"), 0.0, f);
                    }
                }
                _ => job.append(&hex_string(&msg.payload), 0.0, fmt(pal.hex)),
            }
        }
    }
    job
}

fn visible_text(tab: &TabSession) -> String {
    let filtering = !tab.filter_text.is_empty();
    let total = if filtering { tab.filter_cache.ids.len() } else { tab.messages.len() };
    let mut out = String::new();
    for row in 0..total {
        let Some(msg) = message_at(tab, filtering, row) else { continue };
        let tag = match msg.kind {
            MessageKind::Rx => "RX",
            MessageKind::Tx => "TX",
            MessageKind::System => "--",
        };
        let body = match (msg.kind, tab.display_mode, tab.tab_type) {
            (MessageKind::System, _, _) => String::from_utf8_lossy(&msg.payload).to_string(),
            (_, _, TabType::Modbus) | (_, DisplayMode::Hex, _) => hex_string(&msg.payload),
            (_, DisplayMode::Both, _) => {
                format!("{} | {}", printable(&msg.payload), hex_string(&msg.payload))
            }
            (_, DisplayMode::Ascii, _) => String::from_utf8_lossy(&msg.payload).to_string(),
        };
        out.push_str(&format!("{} {tag} {body}\n", msg.timestamp.format("%H:%M:%S%.3f")));
    }
    out
}
