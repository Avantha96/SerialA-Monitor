// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod logger;
mod modbus;
mod serial;
mod stats;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use modbus::{ModbusReply, ModbusRequest};
use serial::{PortInfo, SerialConfig, SerialManager};
use stats::StatsSnapshot;

type Mgr<'a> = State<'a, Arc<SerialManager>>;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SaveResult {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tauri::command]
fn get_ports() -> Vec<PortInfo> {
    SerialManager::list_ports()
}

/// Opening a port can block on a driver, so it runs off the UI thread.
#[tauri::command]
async fn connect(mgr: Mgr<'_>, id: String, config: SerialConfig) -> Result<bool, String> {
    let manager = Arc::clone(mgr.inner());
    tauri::async_runtime::spawn_blocking(move || manager.connect(&id, config))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn disconnect(mgr: Mgr<'_>, id: String) {
    mgr.disconnect(&id);
}

#[tauri::command]
fn set_auto_reconnect(mgr: Mgr<'_>, id: String, enabled: bool) {
    mgr.set_auto_reconnect(&id, enabled);
}

#[tauri::command]
fn write_data(mgr: Mgr<'_>, id: String, data: String, is_hex: bool) {
    mgr.write(&id, &data, is_hex);
}

#[tauri::command]
fn toggle_logger(
    app: AppHandle,
    mgr: Mgr<'_>,
    id: String,
    enable: bool,
) -> Result<Option<String>, String> {
    let documents = app
        .path()
        .document_dir()
        .map_err(|e| format!("cannot resolve Documents folder: {e}"))?;
    mgr.toggle_logger(&id, enable, documents)
}

#[tauri::command]
fn get_stats(mgr: Mgr<'_>, id: String) -> Option<StatsSnapshot> {
    mgr.stats(&id)
}

/// Waits up to 2 s for a slave reply, so it must not run on the UI thread.
#[tauri::command]
async fn modbus_request(mgr: Mgr<'_>, params: ModbusRequest) -> ModbusReply {
    let manager = Arc::clone(mgr.inner());
    match tauri::async_runtime::spawn_blocking(move || manager.modbus_request(&params)).await {
        Ok(reply) => reply,
        Err(e) => ModbusReply::err(format!("Modbus task failed: {e}")),
    }
}

#[tauri::command]
async fn save_file(
    app: AppHandle,
    default_file_name: String,
    content: String,
    filter_name: String,
    extensions: Vec<String>,
) -> SaveResult {
    let docs = app.path().document_dir().ok();
    let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();

    let mut builder = app
        .dialog()
        .file()
        .set_title("Save")
        .set_file_name(default_file_name.as_str())
        .add_filter(filter_name.as_str(), &exts);
    if let Some(dir) = docs {
        builder = builder.set_directory(dir);
    }

    // The picker is callback-based; bridge it to this async command.
    let (tx, rx) = std::sync::mpsc::channel();
    builder.save_file(move |picked| {
        let _ = tx.send(picked.and_then(|fp| fp.into_path().ok()));
    });

    let path: Option<PathBuf> =
        tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
            .await
            .ok()
            .flatten();

    let Some(path) = path else {
        // User cancelled - not an error.
        return SaveResult::default();
    };

    match std::fs::write(&path, content) {
        Ok(()) => SaveResult {
            success: true,
            file_path: Some(path.to_string_lossy().to_string()),
            error: None,
        },
        Err(e) => SaveResult {
            success: false,
            file_path: None,
            error: Some(e.to_string()),
        },
    }
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let manager = Arc::new(SerialManager::new(app.handle().clone()));
            app.manage(manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_ports,
            connect,
            disconnect,
            set_auto_reconnect,
            write_data,
            toggle_logger,
            get_stats,
            modbus_request,
            save_file
        ])
        .build(tauri::generate_context!())
        .expect("failed to start SerialX Monitor");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(mgr) = handle.try_state::<Arc<SerialManager>>() {
                mgr.shutdown();
            }
        }
    });
}
