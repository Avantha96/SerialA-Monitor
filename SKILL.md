# Serial Monitor Development Skill

## Context
This project is a high-performance serial monitor. The user is a firmware engineer and prefers clean, state-driven logic over complex abstractions.

## Architecture (current)
The product is the native Rust + egui app in `src-native/`. The Electron (`electron/`) and
Tauri (`src/`, `src-tauri/`) trees are superseded prototypes. Don't extend them.

## Architectural Rules
1. **Native Rust**: one self-contained executable. Serial I/O uses the `serialport` crate.
2. **Session Isolation**: each tab is its own `TabSession` plus its own port thread. Changing the baud rate in Tab A must not affect Tab B.
3. **Data Integrity**: serial data is handled as raw bytes (`Vec<u8>`), never as strings, so HEX mode is never corrupted. Text is decoded only for display.
4. **Never block the UI thread**: port reads, Modbus waits, disk logging and port enumeration run on background threads and report back through `SerialEvent`s.
5. **State-driven UI**: widgets edit their tab's view state and return an `Actions` value; `app.rs` applies the actions after drawing.
6. **Modbus logic** lives in `modbus.rs` (pure functions) and is covered by the SIL/HIL tests in `tests/`.

## UI Requirements
- Support `System`, `Light`, and `Dark` modes. Take colours from `theme::palette`, never hard-code them in widgets.
- egui's bundled fonts lack some symbols (●, ✓, ✗, →, ➤, ⏹). Paint shapes (`theme::dot`) or use plain text instead.
