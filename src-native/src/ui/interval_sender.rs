//! Send bar: command input with history, line ending, and a repeat timer.

use egui::{Key, RichText, Ui, Vec2};

use super::Actions;
use crate::session::{DisplayMode, LineEnding, TabSession};
use crate::theme::palette;

const MAX_HISTORY: usize = 100;

pub fn render_interval_sender(tab: &mut TabSession, actions: &mut Actions, ui: &mut Ui) {
    let pal = palette(ui);
    let is_hex = tab.display_mode == DisplayMode::Hex;

    if let Some(err) = &tab.send_error {
        ui.colored_label(pal.error, format!("Error: {err}"));
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);

        let hint = if is_hex {
            "Hex bytes, e.g. 01 03 00 00 00 0A — Enter to send"
        } else {
            "Command — Enter to send, Ctrl+Up/Down for history"
        };
        let right_controls = 360.0;
        let input = ui.add(
            egui::TextEdit::singleline(&mut tab.input_text)
                .id(egui::Id::new(("send_input", &tab.id)))
                .hint_text(hint)
                .font(egui::TextStyle::Monospace)
                .desired_width((ui.available_width() - right_controls).max(160.0)),
        );
        if input.changed() {
            tab.send_error = None;
        }

        if input.has_focus() {
            let (up, down) = ui.input(|i| {
                (
                    i.key_pressed(Key::PageUp) || (i.modifiers.ctrl && i.key_pressed(Key::ArrowUp)),
                    i.key_pressed(Key::PageDown) || (i.modifiers.ctrl && i.key_pressed(Key::ArrowDown)),
                )
            });
            if up && !tab.input_history.is_empty() {
                let next = tab.history_idx.map_or(0, |i| (i + 1).min(tab.input_history.len() - 1));
                tab.history_idx = Some(next);
                tab.input_text = tab.input_history[tab.input_history.len() - 1 - next].clone();
            } else if down {
                match tab.history_idx {
                    Some(0) | None => {
                        tab.history_idx = None;
                        tab.input_text.clear();
                    }
                    Some(i) => {
                        tab.history_idx = Some(i - 1);
                        tab.input_text = tab.input_history[tab.input_history.len() - i].clone();
                    }
                }
            }
        }

        let enter = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

        if !is_hex {
            egui::ComboBox::from_id_salt(("ending", &tab.id))
                .selected_text(tab.line_ending.label())
                .width(96.0)
                .show_ui(ui, |ui| {
                    for le in [LineEnding::Nl, LineEnding::Cr, LineEnding::NlCr, LineEnding::None] {
                        ui.selectable_value(&mut tab.line_ending, le, le.label());
                    }
                })
                .response
                .on_hover_text("Appended to every text command");
        }

        let can_send = tab.is_connected && !tab.input_text.trim().is_empty();
        let send_btn = egui::Button::new(RichText::new("Send").strong().color(egui::Color32::WHITE))
            .fill(egui::Color32::from_rgb(37, 99, 235))
            .min_size(Vec2::new(70.0, 24.0));
        let clicked = ui.add_enabled(can_send, send_btn).clicked();

        if (clicked || enter) && can_send {
            let text = tab.input_text.clone();
            actions.send = Some(text.clone());
            if tab.input_history.last() != Some(&text) {
                tab.input_history.push(text);
                if tab.input_history.len() > MAX_HISTORY {
                    tab.input_history.remove(0);
                }
            }
            tab.history_idx = None;
            tab.input_text.clear();
            input.request_focus();
        } else if enter {
            // Keep focus in the box even when there was nothing to send.
            input.request_focus();
        }

        ui.separator();

        ui.add(
            egui::DragValue::new(&mut tab.interval_delay_ms)
                .range(10..=3_600_000)
                .speed(10.0)
                .suffix(" ms"),
        )
        .on_hover_text("Repeat period");

        let (label, fill) = if tab.interval_active {
            ("Stop", egui::Color32::from_rgb(185, 28, 28))
        } else {
            ("⏱ Repeat", egui::Color32::from_rgb(79, 70, 229))
        };
        let btn = egui::Button::new(RichText::new(label).strong().color(egui::Color32::WHITE))
            .fill(fill)
            .min_size(Vec2::new(80.0, 24.0));
        let repeat_source = if tab.input_text.trim().is_empty() {
            tab.input_history.last().cloned()
        } else {
            Some(tab.input_text.clone())
        };
        let hover = match (&tab.interval_active, &repeat_source) {
            (true, _) => format!("Repeating: {}", tab.interval_text),
            (false, Some(s)) => format!("Repeat \"{s}\" every {} ms", tab.interval_delay_ms),
            (false, None) => "Type a command first".to_string(),
        };
        let enabled = tab.is_connected && (tab.interval_active || repeat_source.is_some());
        if ui.add_enabled(enabled, btn).on_hover_text(hover).clicked() {
            if tab.interval_active {
                tab.interval_active = false;
            } else if let Some(src) = repeat_source {
                tab.interval_text = src;
                tab.interval_active = true;
                tab.last_interval_send = None; // fire immediately
            }
        }
        if tab.interval_active {
            ui.label(
                RichText::new(format!("every {} ms: {}", tab.interval_delay_ms, tab.interval_text))
                    .monospace()
                    .color(pal.text_dim),
            );
        }
    });
}
