//! Serial configuration bar: port, line settings, display mode, connect, auto-reconnect, logging.

use egui::{RichText, Ui, Vec2};

use super::Actions;
use crate::serial::PortInfo;
use crate::session::{hex_string, DisplayMode, MessageKind, TabSession, TabType};
use crate::theme::palette;

const BAUDS: [u32; 14] = [
    1200, 2400, 4800, 9600, 14400, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 1_000_000,
    2_000_000,
];

fn caption(ui: &mut Ui, text: &str) {
    let c = palette(ui).text_dim;
    ui.label(RichText::new(text).strong().size(11.0).color(c));
}

pub fn render_config_panel(tab: &mut TabSession, ports: &[PortInfo], actions: &mut Actions, ui: &mut Ui) {
    let pal = palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
        let editable = !tab.is_connected;

        caption(ui, "PORT");
        ui.add_enabled_ui(editable, |ui| {
            let present = ports.iter().any(|p| p.path == tab.config.path);
            let selected = if tab.config.path.is_empty() {
                "Select port".to_string()
            } else if present || tab.is_connected {
                tab.config.path.clone()
            } else {
                format!("{} (absent)", tab.config.path)
            };
            egui::ComboBox::from_id_salt(("port", &tab.id))
                .selected_text(selected)
                .width(130.0)
                .show_ui(ui, |ui| {
                    if ports.is_empty() {
                        ui.label(RichText::new("No serial ports found").color(pal.text_muted));
                    }
                    for port in ports {
                        let label = if port.manufacturer.is_empty() {
                            port.path.clone()
                        } else {
                            format!("{} — {}", port.path, port.manufacturer)
                        };
                        ui.selectable_value(&mut tab.config.path, port.path.clone(), label);
                    }
                });
        });
        if ui.button("🔄").on_hover_text("Rescan serial ports").clicked() {
            actions.refresh_ports = true;
        }

        ui.separator();
        caption(ui, "BAUD");
        ui.add_enabled_ui(editable, |ui| {
            egui::ComboBox::from_id_salt(("baud", &tab.id))
                .selected_text(tab.config.baud_rate.to_string())
                .width(80.0)
                .show_ui(ui, |ui| {
                    for baud in BAUDS {
                        ui.selectable_value(&mut tab.config.baud_rate, baud, baud.to_string());
                    }
                });
            ui.add(
                egui::DragValue::new(&mut tab.config.baud_rate)
                    .range(50..=12_000_000)
                    .speed(100.0),
            )
            .on_hover_text("Custom baud rate (drag or double-click to type)");

            caption(ui, "FRAME");
            egui::ComboBox::from_id_salt(("bits", &tab.id))
                .selected_text(format!("{} data", tab.config.data_bits))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for b in [8u8, 7, 6, 5] {
                        ui.selectable_value(&mut tab.config.data_bits, b, format!("{b} data bits"));
                    }
                });
            egui::ComboBox::from_id_salt(("parity", &tab.id))
                .selected_text(match tab.config.parity.as_str() {
                    "even" => "Even",
                    "odd" => "Odd",
                    _ => "None",
                })
                .width(56.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut tab.config.parity, "none".into(), "None");
                    ui.selectable_value(&mut tab.config.parity, "even".into(), "Even");
                    ui.selectable_value(&mut tab.config.parity, "odd".into(), "Odd");
                });
            egui::ComboBox::from_id_salt(("stop", &tab.id))
                .selected_text(format!("{} stop", tab.config.stop_bits))
                .width(56.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut tab.config.stop_bits, 1, "1 stop bit");
                    ui.selectable_value(&mut tab.config.stop_bits, 2, "2 stop bits");
                });
            ui.checkbox(&mut tab.config.dtr_rts, "DTR/RTS").on_hover_text(
                "Assert DTR and RTS on open.\nLeave off for ESP32/Arduino boards that reset on DTR.",
            );

            if tab.tab_type == TabType::Modbus {
                caption(ui, "TIMEOUT");
                ui.add(
                    egui::DragValue::new(&mut tab.config.response_timeout_ms)
                        .range(20..=10_000)
                        .speed(10.0)
                        .suffix(" ms"),
                )
                .on_hover_text("Modbus response timeout");
            }
        });

        if tab.tab_type == TabType::Generic {
            ui.separator();
            caption(ui, "VIEW");
            ui.selectable_value(&mut tab.display_mode, DisplayMode::Ascii, "ASCII");
            ui.selectable_value(&mut tab.display_mode, DisplayMode::Hex, "HEX");
            ui.selectable_value(&mut tab.display_mode, DisplayMode::Both, "BOTH");
        }

        ui.separator();

        let (text, fill) = if tab.is_connected {
            ("Disconnect", egui::Color32::from_rgb(185, 28, 28))
        } else {
            ("▶ Connect", egui::Color32::from_rgb(5, 150, 105))
        };
        let btn = egui::Button::new(RichText::new(text).strong().color(egui::Color32::WHITE))
            .fill(fill)
            .min_size(Vec2::new(96.0, 24.0));
        if ui.add(btn).clicked() {
            actions.toggle_connect = true;
        }

        let mut auto = tab.auto_reconnect;
        if ui
            .toggle_value(&mut auto, "⟳ Auto-reconnect")
            .on_hover_text("Re-open the port automatically when the device is unplugged and comes back")
            .changed()
        {
            actions.set_auto_reconnect = Some(auto);
        }

        let mut logging = tab.is_logging;
        let log_label = if logging { "💾 Logging…" } else { "💾 Log to disk" };
        let log_hint = match &tab.log_path {
            Some(p) if tab.is_logging => format!("Logging to:\n{p}"),
            _ => format!("Log all traffic to {}", crate::serial::log_dir().display()),
        };
        if ui
            .toggle_value(&mut logging, log_label)
            .on_hover_text(log_hint)
            .changed()
        {
            actions.set_logging = Some(logging);
        }

        if ui
            .button("💾 Export")
            .on_hover_text("Save the on-screen buffer to a text file")
            .clicked()
        {
            export_buffer(tab);
        }
    });
}

fn export_buffer(tab: &mut TabSession) {
    if tab.messages.is_empty() {
        tab.push_system_message("Nothing to export — the buffer is empty.");
        return;
    }
    let default_name = format!(
        "{}_{}.log",
        tab.title().replace(['/', '\\', ':'], "_"),
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Log", &["log", "txt"])
        .set_file_name(default_name)
        .save_file()
    else {
        return;
    };

    let mut content = String::with_capacity(tab.messages.len() * 48);
    for msg in &tab.messages {
        let time = msg.timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
        let (tag, body) = match msg.kind {
            MessageKind::System => ("SYS", String::from_utf8_lossy(&msg.payload).to_string()),
            kind => {
                let tag = if kind == MessageKind::Tx { "TX" } else { "RX" };
                let body = match tab.display_mode {
                    DisplayMode::Hex => hex_string(&msg.payload),
                    DisplayMode::Both => format!(
                        "{} | {}",
                        String::from_utf8_lossy(&msg.payload),
                        hex_string(&msg.payload)
                    ),
                    DisplayMode::Ascii => String::from_utf8_lossy(&msg.payload).to_string(),
                };
                (tag, body)
            }
        };
        content.push_str(&format!("[{time}] {tag} {body}\n"));
    }
    let count = tab.messages.len();
    match std::fs::write(&path, content) {
        Ok(()) => tab.push_system_message(&format!("Exported {count} lines to {}", path.display())),
        Err(e) => tab.push_system_message(&format!("Export failed: {e}")),
    }
}
