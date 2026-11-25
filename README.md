# SerialA Monitor

Multi-session serial + Modbus RTU tool for firmware and field engineers.
A single native Windows executable (Rust + egui): no browser engine, no runtime to install.

The product lives in **`src-native/`**. The `electron/`, `src/` + `src-tauri/` and
`release*/` folders are earlier prototypes (Electron, then Tauri) and are no longer built.

## What you can use it for

| Job | How SerialA helps |
|---|---|
| **Send Modbus RTU commands and decode the replies** | Built-in Modbus master: read/write coils and registers, decode values as INT/UINT/FLOAT with scaling, poll continuously, export to CSV. See [Modbus RTU](#modbus-rtu-send-commands-and-decode-responses). |
| **Capture RS-232 / UART logs** | Timestamped RX/TX logging to disk as text or hex, several ports at once, auto-reconnect when a cable is pulled. See [Capturing RS-232 / serial logs](#capturing-rs-232--serial-logs). |
| **High-speed data streams** | Port reading, line splitting and disk writes run on background threads, so the window stays responsive while the log captures everything. Tested at up to 2 Mbaud with zero lost lines. See [High-speed data](#high-speed-data). |
| **Live debugging of firmware** | Console with filter/regex, Δt between lines, HEX view, start/end-byte highlighting, command history, repeat-send timer and a live plotter for numeric output. |

## Features

- **Multi-session tabs**: any number of ports at once, one reader thread each.
- **Serial console**: ASCII / HEX / both, text or regex filter, Δt between lines,
  start/end-byte (STX/ETX) highlighting, pause, copy, 50,000-line scroll-back per tab.
- **Correct line assembly**: partial lines (prompts, slow senders) show immediately and are
  completed in place when the newline arrives, so output is never split mid-line.
- **Sending**: text with selectable line ending or raw hex, command history, repeat timer.
- **Plotter**: plots numeric lines like `1.2, 3.4` or `temp:21.5 hum:40`, up to 8 channels.
- **Modbus RTU master**: function codes 1/2/3/4/5/6/15/16, single-shot or polling, value decoding, CRC
  check, exception decoding, ok/fail counters, round-trip time, raw frame log, CSV export.
- **Disk logging**: timestamped RX/TX to `Documents\SerialMonitorLogs\<PORT>_<date>_<time>.log`.
- **Auto-reconnect** and a **hot-plug port list** for USB adapters.
- **Full line settings**: any baud rate, 5–8 data bits, none/even/odd parity, 1 or 2 stop bits,
  optional DTR/RTS (off by default so ESP32/Arduino boards don't reset on connect).
- **System / Light / Dark theme**, adjustable font size, and tabs and settings restored on restart.

Shortcuts: `Ctrl+T` new tab · `Ctrl+W` close · `Ctrl+Tab` next · `Ctrl+L` clear ·
`Ctrl +/-/0` font size · `Ctrl+Up/Down` command history.

---

## Modbus RTU: send commands and decode responses

SerialA acts as the Modbus **master** (client): it sends a request to a slave device and
decodes the slave's reply.

### Wiring

Most Modbus RTU devices use RS-485. Connect a USB-to-RS-485 adapter: **A** to A(+), **B** to B(−),
and **GND** to the device's signal ground. On a long bus, terminate both ends with 120 Ω.
Devices with an RS-232 Modbus port connect through a USB-to-RS-232 adapter instead.

### Step by step

1. Click **+** (top right) → **⚡ Modbus RTU**. A new Modbus tab opens.
2. Choose the **PORT**, and set **BAUD**, data bits, parity and stop bits to match the device.
   These must match exactly; common settings are 9600 8N1, 9600 8E1 and 19200 8E1.
3. Set **TIMEOUT** (default 2000 ms): how long to wait for a reply.
4. Click **▶ Connect**.
5. In the Modbus bar, set:
   - **Slave**: the device address (1–247).
   - **Function**: what to do (see the table below).
   - **Address**: the first register or coil, entered in hex. Hover over the field to see the decimal value.
   - **Count** (for reads): how many registers (max 125) or coils/inputs (max 2000).
   - **Value** (for writes): see the table below.
6. Click **⚡ Send** to send one request, or set an interval (e.g. `1000 ms`) and click **🔁 Poll**
   to repeat it.

| Function | Use | What to enter in **Value** |
|---|---|---|
| 01 Read Coils | read output bits | — |
| 02 Read Discrete Inputs | read input bits | — |
| 03 Read Holding Registers | read settings / measurements | — |
| 04 Read Input Registers | read measurements | — |
| 05 Write Single Coil | switch one output | `1` (ON) or `0` (OFF) |
| 06 Write Single Register | write one 16-bit value | e.g. `1234` (negative numbers are sent as two's complement) |
| 15 Write Multiple Coils | switch several outputs | comma list, e.g. `1,0,1,1` |
| 16 Write Multiple Registers | write several values | comma list, e.g. `100,200,300` |

Write values are **whole numbers**. Anything that isn't a number is sent as `0`, so double-check
before sending a write. To write a FLOAT32, use function 16 with the float's two raw 16-bit words.

### Decoding the response

The register table shows every value as raw hex, decoded value, and binary. Use **Type** and the **×**
(scale) field to decode registers as engineering values:

| Type | Meaning |
|---|---|
| UINT16 / INT16 | one register, unsigned / signed |
| UINT32 / INT32 (AB CD) | two registers, high word first |
| FLOAT32 (AB CD) | IEEE-754 float, high word first |
| FLOAT32 (CD AB) | IEEE-754 float, low word first ("word swapped"). Try this if floats look wrong. |

For 32-bit types the value is shown on the first register of each pair, with `^` on the second.

**Example**: reading phase voltages from an Acrel ADL400 meter (register map in
`src-native/src/adl400.rs`): Slave `1`, Function `03`, Address `0x0039`, Count `3`, Type `UINT16`,
× `0.1`. The Value column then shows Ua/Ub/Uc in volts (e.g. `2301` → `230.1`).

Other information shown:
- **Errors in plain words**: timeouts, CRC mismatch, wrong slave address, incomplete replies, and
  Modbus exceptions by name (e.g. *Illegal data address*).
- **ok / fail counters** and **RTT** (round-trip time) of the last request.
- **Bus traffic** panel (bottom): every request (TX) and reply (RX) frame in hex, for
  comparison with a protocol manual. **Log to disk** saves these frames too.
- **📥 CSV** exports the register table.

### Sending a raw Modbus frame (advanced)

For hand-built frames, use a **Generic Serial** tab in **HEX** view and type the full frame,
*including the CRC* (low byte first). Example, read 10 holding registers from slave 1:

```
01 03 00 00 00 0A C5 CD
```

The reply appears in hex. A generic tab splits incoming data at `0x0A` bytes, so binary replies can
appear across several lines. The Modbus tab doesn't have that problem and checks the CRC for you,
so prefer it.

### Limits

- Master only: SerialA doesn't act as a slave, and it doesn't decode traffic between *other*
  masters and slaves on a bus.
- Modbus **RTU** only (not Modbus ASCII or Modbus TCP).

---

## Capturing RS-232 / serial logs

### Step by step

1. Plug in the USB-to-serial adapter. It appears in the **PORT** list automatically.
2. Open a **Generic Serial** tab (the default tab), pick the port, and set baud, data bits,
   parity and stop bits to match the device.
3. Pick the view:
   - **ASCII** for text output (debug prints, AT commands, NMEA, …)
   - **HEX** for binary protocols
4. Click **▶ Connect**, then **💾 Log to disk**. The status bar shows the log file path; click it to copy.
5. Leave it running. Click **💾 Log to disk** again (or disconnect) to close the file.

If you turn logging on before connecting, it starts on connect. With **Auto-reconnect** on, a
pulled cable doesn't end the capture: logging resumes in a new file when the port comes back.

### Log file format

Files go to `Documents\SerialMonitorLogs\` and are named after the port and start time
(`COM9_2026-11-25_00-01-22.log`). One entry per line, millisecond timestamps, direction tag:

```
[2026-11-25 00:01:23.069] TX hello from SerialA
[2026-11-25 00:01:23.086] RX Queue received: h
[2026-11-25 00:01:23.119] RX Blink task: LED off
```

In **HEX** view, RX data is logged as hex bytes. This applies immediately when you switch the view, so
binary data never corrupts the text file:

```
[2026-11-25 00:17:05.774] RX 51 75 65 75 65 20 72 65 63 65 69 76 65 64 3A 20 68
```

Lines that arrive in the same read share one timestamp.

### Tips

- **⏸ Pause** freezes the screen for reading; the disk log keeps recording.
- **💾 Export** saves only what's in the on-screen buffer (last 50,000 lines). Use **Log to disk**
  for a complete capture.
- Open one tab per port to capture several devices at once; each tab gets its own log file.
- **Listening in on a link between two other devices:** wire an RS-232 tap so your adapter's
  **RX** connects to the line you want to hear (plus GND), and leave your adapter's TX unconnected.
  To capture both directions, use two adapters (one per direction) in two tabs; the timestamps line
  them up.
- **Voltage levels:** true RS-232 swings to ±12 V; a microcontroller UART is 3.3 V/5 V TTL. Use
  an RS-232 adapter for RS-232 and a TTL adapter for UART pins. Mixing them up can damage the board.

---

## High-speed data

SerialA is built so the window never slows down the capture:

- Each port has its own **reader thread** that reads up to 16 KiB per call and splits lines there,
  not in the UI.
- **Disk logging runs on a separate thread** with a 256 KiB write buffer flushed every 500 ms, so
  a busy port never waits on the disk.
- The screen keeps the last **50,000 lines per tab** in a ring buffer and draws only the visible
  rows. Filtering and plotting process only new lines.
- The UI redraws only when data arrives (about 1% CPU when idle).
- Modbus waits and port scanning also run off the UI thread.

Tips for fast streams:
- Turn on **Log to disk**: the file gets every byte, even when the screen buffer rolls over or is
  paused.
- Use the **filter** to watch for specific messages while everything else is still logged.
- Close the **📈 Plot** and **📊 Stats** panels if you don't need them.
- The highest usable baud rate depends on your USB adapter. Any rate can be typed in the field
  next to the BAUD list.

### Measured throughput

Measured on an Arduino Nano (ATmega328P, CH340 USB-serial) running the test firmware in
`firmware/hs_stream`, 5 s per rate. Data went through SerialA's real processing path (reader thread,
line splitting, screen buffer, disk log), and every line was checked by sequence number and checksum:

| Baud | Lines/s | Bytes/s | Line utilisation | Lost | Corrupted | Log file | App cost per line |
|---:|---:|---:|---:|---:|---:|---|---:|
| 115 200 | 209 | 11 707 | 102 %¹ | 0 | 0 | complete | 10 µs |
| 250 000 | 441 | 24 723 | 99 % | 0 | 0 | complete | 12 µs |
| 500 000 | 848 | 47 471 | 95 % | 0 | 0 | complete | 9 µs |
| 1 000 000 | 1 571 | 87 973 | 88 % | 0 | 0 | complete | 8 µs |
| 2 000 000 | 2 738 | 153 354 | 77 % | 0 | 0 | complete | 7 µs |

¹ A 16 MHz AVR can't make exactly 115 200 baud; it runs 2.1 % fast (117 647).

At every rate, SerialA received and logged every line. Below 100% utilisation, the limit is the
Nano itself: it can't build lines faster than that. SerialA's own processing costs about 7–12 µs
per line (about 2% of one CPU core at 2,700 lines/s). Screen drawing isn't included in that
figure; it's limited to the visible rows and at most one redraw per frame.

### Test it yourself

The test firmware and the measuring tool are part of this repository:

```
cd firmware
./build_upload.sh hs_stream COM9          # build + upload to a Nano/Uno (overwrites its sketch)

cd ../src-native
cargo run --release --example hs_test -- COM9 5                 # all rates, 5 s each
cargo run --release --example hs_test -- COM9 10 2000000        # one rate, 10 s
```

`build_upload.sh` uses the compiler and uploader installed with the Arduino IDE / Boards Manager
(`%LOCALAPPDATA%\Arduino15`); the IDE itself isn't needed. Alternatively, open
`firmware/hs_stream/hs_stream.ino` in the Arduino IDE and upload it as usual.

To watch the stream in the app: open a Generic tab, select the port, set **BAUD** to `1000000`
(the firmware's power-on rate), and connect. The firmware accepts commands from the send box:

| Command | Effect |
|---|---|
| `S` / `G` | stop / start streaming |
| `R` | reset the sequence number to 0 |
| `D500` | wait 500 µs between lines (`D0` = full speed) |
| `B2000000` | switch to 2 Mbaud (then disconnect, change BAUD in SerialA, reconnect) |
| `?` | print status |

Line format: `$SSSSSSSS,TTTTTTTT,<32 chars>*CC`, with a hex sequence number, the board's
`micros()`, a rotating payload, and an XOR checksum of the characters between `$` and `*`. A skipped
sequence number means lost data.

---

## Build & run

Requires Rust stable (`rustup`). The GNU (`x86_64-pc-windows-gnu`) and MSVC toolchains both work.

```
cd src-native
cargo run --release                          # build and start the app
cargo test                                   # unit + Modbus SIL/HIL simulation tests
cargo run --example hw_probe -- COM9 9600    # smoke-test a real device from the command line
```

The release binary is `src-native/target/release/seriala-native.exe` (about 6 MB, self-contained).
Copy it anywhere and run it.

## Architecture

```
firmware/
  hs_stream/hs_stream.ino   high-speed test stream for Arduino Nano/Uno
  build_upload.sh           build + upload a sketch without the Arduino IDE
src-native/examples/
  hs_test.rs                throughput / data-integrity test against hs_stream
  hw_probe.rs               quick connect/read/write/log check against any device
src-native/src/
  modbus.rs    RTU frame encode/decode + CRC-16          (pure, unit-tested)
  adl400.rs    Acrel ADL400 meter simulator for SIL/HIL tests
  stats.rs     per-session byte counters and rates
  logger.rs    background log-writer thread
  serial.rs    SerialManager: port threads, reconnect, port watcher, Modbus transactions
  session.rs   per-tab state: ring buffer, incremental filter/plot caches, saved settings
  theme.rs     System/Light/Dark palettes
  ui/          egui widgets; they return an `Actions` value instead of touching ports
  app.rs       event loop: drain serial events -> apply timers -> draw -> apply actions
```

Data flow: each open port has a reader thread that splits lines and posts `SerialEvent`s to
a channel. Posting also wakes the UI, so **the UI repaints only when there is something new**.
Disk logging, port enumeration and Modbus request/response waits all run on their own threads,
so a slow device or disk never freezes the window.
