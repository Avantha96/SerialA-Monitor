//! System / Light / Dark theming.
//!
//! UI code never hard-codes surface or text colours; it asks for a [`Palette`]
//! derived from the active egui visuals, so both themes stay consistent.
//! Accent colours (RX green, TX blue, …) are picked per theme for contrast.

use egui::{Color32, Context, Theme, ThemePreference, Ui, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }
    fn preference(self) -> ThemePreference {
        match self {
            ThemeChoice::System => ThemePreference::System,
            ThemeChoice::Light => ThemePreference::Light,
            ThemeChoice::Dark => ThemePreference::Dark,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_muted: Color32,
    pub border: Color32,
    pub rx: Color32,
    pub tx: Color32,
    pub sys: Color32,
    pub hex: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub ok: Color32,
    pub accent: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(17, 24, 39),
    panel: Color32::from_rgb(31, 41, 55),
    text: Color32::from_rgb(229, 231, 235),
    text_dim: Color32::from_rgb(156, 163, 175),
    text_muted: Color32::from_rgb(107, 114, 128),
    border: Color32::from_rgb(75, 85, 99),
    rx: Color32::from_rgb(52, 211, 153),
    tx: Color32::from_rgb(96, 165, 250),
    sys: Color32::from_rgb(251, 191, 36),
    hex: Color32::from_rgb(147, 197, 253),
    warn: Color32::from_rgb(217, 119, 6),
    error: Color32::from_rgb(248, 113, 113),
    ok: Color32::from_rgb(16, 185, 129),
    accent: Color32::from_rgb(52, 211, 153),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(249, 250, 251),
    panel: Color32::from_rgb(229, 231, 235),
    text: Color32::from_rgb(17, 24, 39),
    text_dim: Color32::from_rgb(75, 85, 99),
    text_muted: Color32::from_rgb(107, 114, 128),
    border: Color32::from_rgb(209, 213, 219),
    rx: Color32::from_rgb(4, 120, 87),
    tx: Color32::from_rgb(29, 78, 216),
    sys: Color32::from_rgb(180, 83, 9),
    hex: Color32::from_rgb(30, 64, 175),
    warn: Color32::from_rgb(180, 83, 9),
    error: Color32::from_rgb(185, 28, 28),
    ok: Color32::from_rgb(5, 150, 105),
    accent: Color32::from_rgb(5, 150, 105),
};

pub fn palette(ui: &Ui) -> Palette {
    if ui.visuals().dark_mode {
        DARK
    } else {
        LIGHT
    }
}

pub fn palette_ctx(ctx: &Context) -> Palette {
    if ctx.style().visuals.dark_mode {
        DARK
    } else {
        LIGHT
    }
}

/// A filled status dot. Painted rather than typed: egui's bundled fonts lack '●'.
pub fn dot(ui: &mut Ui, color: Color32) -> egui::Response {
    let size = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size * 0.7, size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size * 0.28, color);
    resp
}

/// Frame for the toolbar strips (config bar, send bar).
pub fn bar_frame(ctx: &Context) -> egui::Frame {
    egui::Frame::none()
        .fill(palette_ctx(ctx).panel)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
}

/// Install tuned visuals for both themes once at start-up.
pub fn install(ctx: &Context, choice: ThemeChoice) {
    let mut dark = Visuals::dark();
    dark.panel_fill = DARK.bg;
    dark.window_fill = DARK.bg;
    dark.faint_bg_color = DARK.panel;
    dark.extreme_bg_color = Color32::from_rgb(10, 15, 29);
    dark.widgets.noninteractive.bg_fill = DARK.panel;
    dark.widgets.inactive.bg_fill = DARK.panel;
    dark.widgets.hovered.bg_fill = Color32::from_rgb(55, 65, 81);
    dark.widgets.active.bg_fill = DARK.border;
    ctx.set_visuals_of(Theme::Dark, dark);

    let mut light = Visuals::light();
    light.panel_fill = LIGHT.bg;
    light.window_fill = LIGHT.bg;
    light.faint_bg_color = Color32::from_rgb(243, 244, 246);
    light.extreme_bg_color = Color32::WHITE;
    ctx.set_visuals_of(Theme::Light, light);

    set(ctx, choice);
}

pub fn set(ctx: &Context, choice: ThemeChoice) {
    ctx.set_theme(choice.preference());
}
