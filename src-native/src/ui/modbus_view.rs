//! Modbus RTU master test bench: request builder, polling, and decoded register table.

use egui::{RichText, ScrollArea, Ui, Vec2};
use egui_extras::{Column, TableBuilder};

use super::Actions;
use crate::session::{RegFormat, RowStatus, TabSession};
use crate::theme::palette;

const FUNCTIONS: [(u8, &str); 8] = [
    (1, "01 Read Coils"),
    (2, "02 Read Discrete Inputs"),
    (3, "03 Read Holding Registers"),
    (4, "04 Read Input Registers"),
    (5, "05 Write Single Coil"),
    (6, "06 Write Single Register"),
    (15, "15 Write Multiple Coils"),
    (16, "16 Write Multiple Registers"),
];

fn is_bit_fc(fc: u8) -> bool {
    matches!(fc, 1 | 2 | 5 | 15)
}

pub fn render_modbus_toolbar(tab: &mut TabSession, actions: &mut Actions, ui: &mut Ui) {
    let pal = palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);

        ui.label("Slave");
        ui.add(egui::DragValue::new(&mut tab.modbus_req.slave_id).range(1..=247));

        let fc = tab.modbus_req.function_code;
        let fc_label = FUNCTIONS.iter().find(|(c, _)| *c == fc).map_or("?", |(_, l)| l);
        egui::ComboBox::from_id_salt(("fc", &tab.id))
            .selected_text(fc_label)
            .width(200.0)
            .show_ui(ui, |ui| {
                for (code, label) in FUNCTIONS {
                    ui.selectable_value(&mut tab.modbus_req.function_code, code, label);
                }
            });

        ui.label("Address");
        let mut addr = tab.modbus_req.start_address;
        if ui
            .add(egui::DragValue::new(&mut addr).hexadecimal(4, false, true).prefix("0x"))
            .on_hover_text(format!("Decimal {addr}"))
            .changed()
        {
            tab.modbus_req.start_address = addr;
        }

        let fc = tab.modbus_req.function_code;
        let is_write = matches!(fc, 5 | 6 | 15 | 16);
        if is_write {
            ui.label("Value");
            let hint = match fc {
                5 => "1 or 0",
                6 => "e.g. 1234",
                15 => "1,0,1,1",
                _ => "100,200,300",
            };
            ui.add(
                egui::TextEdit::singleline(&mut tab.modbus_req.write_value)
                    .hint_text(hint)
                    .desired_width(120.0),
            );
            let n = match fc {
                15 | 16 => tab.modbus_req.write_value.split(',').filter(|s| !s.trim().is_empty()).count(),
                _ => 1,
            };
            tab.modbus_req.quantity = n.max(1) as u16;
        } else {
            ui.label("Count");
            let max = if is_bit_fc(fc) { 2000 } else { 125 };
            let mut qty = tab.modbus_req.quantity.min(max);
            ui.add(egui::DragValue::new(&mut qty).range(1..=max));
            tab.modbus_req.quantity = qty;
        }
        tab.sync_modbus_rows();

        if !is_bit_fc(fc) {
            ui.separator();
            ui.label("Type");
            egui::ComboBox::from_id_salt(("fmt", &tab.id))
                .selected_text(tab.reg_format.label())
                .width(120.0)
                .show_ui(ui, |ui| {
                    for f in RegFormat::ALL {
                        ui.selectable_value(&mut tab.reg_format, f, f.label());
                    }
                });
            ui.label("×");
            ui.add(egui::DragValue::new(&mut tab.scale).speed(0.01).max_decimals(6))
                .on_hover_text("Scale factor, e.g. 0.1 for a register in tenths of a volt");
        }

        ui.separator();

        let send = egui::Button::new(RichText::new("⚡ Send").strong().color(egui::Color32::WHITE))
            .fill(egui::Color32::from_rgb(147, 51, 234))
            .min_size(Vec2::new(70.0, 24.0));
        if ui
            .add_enabled(tab.is_connected && !tab.modbus_in_flight, send)
            .on_disabled_hover_text(if tab.is_connected { "Waiting for the previous response" } else { "Connect first" })
            .clicked()
        {
            actions.modbus_send = true;
        }

        ui.add(
            egui::DragValue::new(&mut tab.modbus_poll_interval_ms)
                .range(20..=60_000)
                .speed(10.0)
                .suffix(" ms"),
        )
        .on_hover_text("Poll period");
        let (label, fill) = if tab.modbus_polling {
            ("Stop", egui::Color32::from_rgb(185, 28, 28))
        } else {
            ("🔁 Poll", egui::Color32::from_rgb(5, 150, 105))
        };
        let poll = egui::Button::new(RichText::new(label).strong().color(egui::Color32::WHITE)).fill(fill);
        if ui.add_enabled(tab.is_connected || tab.modbus_polling, poll).clicked() {
            tab.modbus_polling = !tab.modbus_polling;
            tab.last_modbus_poll = None;
        }

        if ui.button("📥 CSV").on_hover_text("Export the register table").clicked() {
            export_csv(tab);
        }
    });

    ui.horizontal(|ui| {
        if let Some(err) = &tab.modbus_error {
            ui.colored_label(pal.error, format!("Error: {err}"));
        } else if let Some(time) = &tab.modbus_last_time {
            ui.colored_label(pal.ok, format!("OK — response at {time}"));
        } else {
            ui.label(RichText::new("No request sent yet").color(pal.text_muted));
        }
        if tab.modbus_in_flight {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Reset").on_hover_text("Reset counters").clicked() {
                tab.modbus_ok = 0;
                tab.modbus_fail = 0;
                tab.modbus_last_rtt_ms = None;
            }
            if let Some(rtt) = tab.modbus_last_rtt_ms {
                ui.label(RichText::new(format!("RTT {rtt} ms")).color(pal.text_dim));
            }
            ui.colored_label(pal.error, format!("fail {}", tab.modbus_fail));
            ui.colored_label(pal.ok, format!("ok {}", tab.modbus_ok));
        });
    });
}

fn display_value(tab: &TabSession, regs: &[Option<i64>], i: usize) -> String {
    let fc = tab.modbus_req.function_code;
    if is_bit_fc(fc) {
        return match regs[i] {
            Some(v) => if v != 0 { "ON (1)".into() } else { "OFF (0)".into() },
            None => "–".into(),
        };
    }
    if tab.reg_format.is_32bit() && i % 2 == 1 {
        return "^".into();
    }
    match tab.reg_format.value(regs, i) {
        Some(v) => {
            let scaled = v * tab.scale;
            if tab.scale == 1.0 && !matches!(tab.reg_format, RegFormat::F32 | RegFormat::F32Swapped) {
                format!("{scaled:.0}")
            } else {
                let s = format!("{scaled:.6}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
        None => "–".into(),
    }
}

pub fn render_modbus_table(tab: &TabSession, ui: &mut Ui) {
    let pal = palette(ui);
    let regs: Vec<Option<i64>> = tab.modbus_rows.iter().map(|r| r.raw).collect();
    let bits = is_bit_fc(tab.modbus_req.function_code);

    ScrollArea::horizontal().show(ui, |ui| {
        TableBuilder::new(ui)
            .id_salt(("mb_table", &tab.id))
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::auto().at_least(110.0))
            .column(Column::auto().at_least(70.0))
            .column(Column::auto().at_least(150.0))
            .column(Column::auto().at_least(150.0))
            .column(Column::remainder().at_least(60.0))
            .header(22.0, |mut h| {
                for title in ["Address", "Raw", "Value", "Binary", "Status"] {
                    h.col(|ui| {
                        ui.strong(title);
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, tab.modbus_rows.len(), |mut row| {
                    let i = row.index();
                    let r = &tab.modbus_rows[i];
                    row.col(|ui| {
                        ui.label(RichText::new(format!("0x{:04X}  {}", r.address, r.address)).monospace());
                    });
                    row.col(|ui| {
                        let raw = match r.raw {
                            Some(v) if bits => v.to_string(),
                            Some(v) => format!("0x{v:04X}"),
                            None => "–".into(),
                        };
                        ui.label(RichText::new(raw).monospace().color(pal.hex));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(display_value(tab, &regs, i)).monospace().strong().color(pal.rx));
                    });
                    row.col(|ui| {
                        let bin = match r.raw {
                            Some(v) if !bits => {
                                let s = format!("{:016b}", v as u16);
                                format!("{} {} {} {}", &s[0..4], &s[4..8], &s[8..12], &s[12..16])
                            }
                            _ => String::new(),
                        };
                        ui.label(RichText::new(bin).monospace().color(pal.text_dim));
                    });
                    row.col(|ui| {
                        let (text, color) = match r.status {
                            RowStatus::Ok => ("OK", pal.ok),
                            RowStatus::Error => ("ERR", pal.error),
                            RowStatus::Pending => ("…", pal.warn),
                            RowStatus::Idle => ("—", pal.text_muted),
                        };
                        ui.colored_label(color, text);
                    });
                });
            });
    });
}

fn export_csv(tab: &mut TabSession) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("CSV", &["csv"])
        .set_file_name(format!("modbus_slave{}_{}.csv", tab.modbus_req.slave_id, chrono::Local::now().format("%Y%m%d_%H%M%S")))
        .save_file()
    else {
        return;
    };
    let regs: Vec<Option<i64>> = tab.modbus_rows.iter().map(|r| r.raw).collect();
    let mut csv = format!("Address,Raw,Value ({} x{}),Status\n", tab.reg_format.label(), tab.scale);
    for (i, r) in tab.modbus_rows.iter().enumerate() {
        let raw = r.raw.map(|v| v.to_string()).unwrap_or_default();
        let val = display_value(tab, &regs, i);
        let val = if val == "–" || val == "^" { String::new() } else { val };
        let status = match r.status {
            RowStatus::Ok => "OK",
            RowStatus::Error => "ERROR",
            RowStatus::Pending => "PENDING",
            RowStatus::Idle => "",
        };
        csv.push_str(&format!("{},{raw},{val},{status}\n", r.address));
    }
    match std::fs::write(&path, csv) {
        Ok(()) => tab.push_system_message(&format!("Exported register table to {}", path.display())),
        Err(e) => tab.push_system_message(&format!("CSV export failed: {e}")),
    }
}
