//! Tab header bar with session selector, connection indicators, add-tab menu and view settings.

use egui::{RichText, Stroke, Ui, Vec2};

use super::Actions;
use crate::session::{TabSession, TabType};
use crate::theme::{dot, palette, ThemeChoice};

pub fn render_tab_bar(
    tabs: &[TabSession],
    active_tab_id: Option<&str>,
    font_size: f32,
    theme: ThemeChoice,
    actions: &mut Actions,
    ui: &mut Ui,
) {
    let pal = palette(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);

        ui.add_space(8.0);
        ui.label(RichText::new("⚡ SerialA").size(font_size + 2.0).strong().color(pal.accent));
        ui.add_space(12.0);

        // Right-hand controls are laid out first so the tab strip gets the remaining width.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            ui.menu_button("⚙", |ui| {
                ui.set_min_width(180.0);
                ui.label(RichText::new("Theme").strong());
                for choice in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
                    if ui.radio(theme == choice, choice.label()).clicked() {
                        actions.set_theme = Some(choice);
                        ui.close_menu();
                    }
                }
                ui.separator();
                ui.label(RichText::new(format!("Font size: {font_size:.0}")).strong());
                ui.horizontal(|ui| {
                    if ui.button("A-").clicked() {
                        actions.font_delta = Some(-1.0);
                    }
                    if ui.button("A+").clicked() {
                        actions.font_delta = Some(1.0);
                    }
                });
                ui.separator();
                ui.label(RichText::new("Shortcuts").strong());
                for (k, v) in [
                    ("Ctrl+T", "New serial tab"),
                    ("Ctrl+W", "Close tab"),
                    ("Ctrl+Tab", "Next tab"),
                    ("Ctrl+L", "Clear console"),
                    ("Ctrl + / - / 0", "Font size"),
                    ("Ctrl+Up/Down", "Command history"),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(k).monospace().color(pal.text_dim));
                        ui.label(v);
                    });
                }
            });

            ui.menu_button(RichText::new(" + ").strong(), |ui| {
                ui.set_min_width(150.0);
                if ui.button("🖥 Generic Serial").clicked() {
                    actions.add_tab = Some(TabType::Generic);
                    ui.close_menu();
                }
                if ui.button("⚡ Modbus RTU").clicked() {
                    actions.add_tab = Some(TabType::Modbus);
                    ui.close_menu();
                }
            })
            .response
            .on_hover_text("New session");

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for tab in tabs {
                                render_tab(tab, active_tab_id == Some(tab.id.as_str()), font_size, actions, ui);
                                ui.add_space(2.0);
                            }
                        });
                    });
            });
        });
    });
}

fn render_tab(tab: &TabSession, is_active: bool, font_size: f32, actions: &mut Actions, ui: &mut Ui) {
    let pal = palette(ui);
    let frame = egui::Frame::none()
        .fill(if is_active { pal.panel } else { pal.bg })
        .stroke(if is_active { Stroke::new(1.0_f32, pal.border) } else { Stroke::NONE })
        .rounding(egui::Rounding { nw: 6.0, ne: 6.0, sw: 0.0, se: 0.0 })
        .inner_margin(egui::Margin::symmetric(8.0, 6.0));

    // The whole tab is clickable, but the click area is registered *behind* its children
    // (UiBuilder::sense), so the × button on top still receives its own clicks.
    // Registering it afterwards (Response::interact) would cover the × and swallow them.
    let tab_area = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                dot(ui, if tab.is_connected { pal.ok } else { pal.text_muted });
                let (icon, color) = match tab.tab_type {
                    TabType::Generic => ("🖥", pal.tx),
                    TabType::Modbus => ("⚡", egui::Color32::from_rgb(168, 85, 247)),
                };
                ui.colored_label(color, icon);
                if tab.is_logging {
                    ui.colored_label(pal.error, "REC").on_hover_text("Logging to disk");
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(tab.title())
                            .size(font_size)
                            .color(if is_active { pal.text } else { pal.text_dim }),
                    )
                    .selectable(false),
                );
                if ui.small_button("×").on_hover_text("Close session").clicked() {
                    actions.close_tab = Some(tab.id.clone());
                }
            });
        });
    });
    if tab_area.response.clicked() && actions.close_tab.as_deref() != Some(tab.id.as_str()) {
        actions.activate_tab = Some(tab.id.clone());
    }
}
