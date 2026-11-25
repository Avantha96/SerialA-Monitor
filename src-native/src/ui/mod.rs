pub mod config_panel;
pub mod console_view;
pub mod interval_sender;
pub mod modbus_view;
pub mod plotter_view;
pub mod stats_panel;
pub mod status_bar;
pub mod tab_bar;

use crate::session::TabType;
use crate::theme::ThemeChoice;

/// Requests raised by UI widgets during a frame, applied by the app afterwards.
/// Widgets only edit their own tab's view state; anything touching ports goes through here.
#[derive(Default)]
pub struct Actions {
    pub refresh_ports: bool,
    pub toggle_connect: bool,
    pub set_auto_reconnect: Option<bool>,
    pub set_logging: Option<bool>,
    pub send: Option<String>,
    pub modbus_send: bool,
    pub add_tab: Option<TabType>,
    pub close_tab: Option<String>,
    pub activate_tab: Option<String>,
    pub set_theme: Option<ThemeChoice>,
    pub font_delta: Option<f32>,
}
