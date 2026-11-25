//! High-speed throughput test against `firmware/hs_stream` running on a board.
//!
//!   cargo run --release --example hs_test -- COM9 [seconds] [baud ...]
//!
//! Runs the stream through SerialA's real pipeline (SerialManager reader thread ->
//! event channel -> TabSession ring buffer, with disk logging on) and checks every
//! line's sequence number and XOR checksum. Reports lines/s, bytes/s, lost and
//! corrupted lines, the app-side processing cost, and whether the log file is complete.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use seriala_native::serial::{SerialConfig, SerialEvent, SerialManager};
use seriala_native::session::{TabSession, TabType};

const BOARD_DEFAULT_BAUD: u32 = 1_000_000;

#[derive(Default, Debug)]
struct Report {
    lines: u64,
    bytes: u64,
    bad_checksum: u64,
    malformed: u64,
    lost: u64,
    info: Vec<String>,
    process_time: Duration,
}

/// Validate "$SSSSSSSS,TTTTTTTT,<32>*CC". Returns the sequence number if valid.
fn check_line(line: &[u8]) -> Result<u32, &'static str> {
    // 1 '$' + 8 seq + ',' + 8 time + ',' + 32 payload + '*' + 2 checksum = 54
    if line.len() != 54 || line[0] != b'$' || line[51] != b'*' {
        return Err("malformed");
    }
    let x = line[1..51].iter().fold(0u8, |a, &b| a ^ b);
    let cc = std::str::from_utf8(&line[52..54]).ok().and_then(|s| u8::from_str_radix(s, 16).ok());
    if cc != Some(x) {
        return Err("checksum");
    }
    std::str::from_utf8(&line[1..9])
        .ok()
        .and_then(|s| u32::from_str_radix(s, 16).ok())
        .ok_or("malformed")
}

fn drain(rx: &mpsc::Receiver<SerialEvent>, dur: Duration) {
    let end = Instant::now() + dur;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if rx.recv_timeout(left).is_err() {
            break;
        }
    }
}

fn open(mgr: &Arc<SerialManager>, port: &str, baud: u32) {
    let cfg = SerialConfig { path: port.into(), baud_rate: baud, ..Default::default() };
    mgr.connect("hs", cfg).unwrap_or_else(|e| panic!("open {port} @ {baud}: {e}"));
}

/// Ask the board (currently at `from`) to switch to `to`, and reopen the port at `to`.
fn switch_baud(mgr: &Arc<SerialManager>, rx: &mpsc::Receiver<SerialEvent>, port: &str, from: u32, to: u32) {
    open(mgr, port, from);
    mgr.write("hs", b"S\n", false).unwrap();
    drain(rx, Duration::from_millis(100));
    mgr.write("hs", format!("B{to}\n").as_bytes(), false).unwrap();
    drain(rx, Duration::from_millis(100));
    mgr.disconnect("hs");
    drain(rx, Duration::from_millis(200));
}

fn run(mgr: &Arc<SerialManager>, rx: &mpsc::Receiver<SerialEvent>, port: &str, baud: u32, secs: u64) -> (Report, String, f64) {
    open(mgr, port, baud);
    mgr.write("hs", b"S\nR\n", false).unwrap();
    drain(rx, Duration::from_millis(150));
    let log_path = mgr.set_logging("hs", true, &format!("hs_{baud}")).unwrap().unwrap();

    let mut tab = TabSession::new(1, TabType::Generic);
    let mut rep = Report::default();
    let mut expect: Option<u32> = None;

    mgr.write("hs", b"G\n", false).unwrap();
    let start = Instant::now();
    let end = start + Duration::from_secs(secs);
    let mut first_data: Option<Instant> = None;
    let mut last_data = start;
    let mut window_bytes = 0u64; // bytes received before the stop command, for the rate
    let mut stop_sent = false;
    loop {
        // Measure for `secs`, then stop the board and keep counting until it goes quiet,
        // so the received-line count is comparable with the log file.
        if !stop_sent && Instant::now() >= end {
            mgr.write("hs", b"S\n", false).unwrap();
            stop_sent = true;
        }
        if Instant::now() >= end + Duration::from_secs(3) {
            println!("          (board did not stop streaming; ending the run)");
            break;
        }
        let Ok(ev) = rx.recv_timeout(Duration::from_millis(300)) else {
            if stop_sent {
                break;
            }
            continue;
        };
        if let SerialEvent::Data { lines, partial, .. } = ev {
            first_data.get_or_insert_with(Instant::now);
            if !stop_sent {
                last_data = Instant::now();
                window_bytes += lines.iter().map(|l| l.len() as u64 + 2).sum::<u64>();
            }
            let t = Instant::now();
            for line in &lines {
                rep.bytes += line.len() as u64 + 2;
                if line.first() == Some(&b'#') {
                    rep.info.push(String::from_utf8_lossy(line).to_string());
                    continue;
                }
                rep.lines += 1;
                match check_line(line) {
                    Ok(seq) => {
                        if let Some(e) = expect {
                            if seq > e {
                                rep.lost += (seq - e) as u64;
                            }
                        }
                        expect = Some(seq.wrapping_add(1));
                    }
                    Err("checksum") => rep.bad_checksum += 1,
                    Err(_) => rep.malformed += 1,
                }
            }
            // Same processing the app does on the UI thread.
            tab.push_rx_batch(lines, partial);
            rep.process_time += t.elapsed();
        }
    }
    let window = first_data.map_or(0.0, |f| (last_data - f).as_secs_f64());
    rep.bytes = window_bytes; // throughput is measured over the timed window only
    let _ = mgr.set_logging("hs", false, "");
    mgr.disconnect("hs");
    drain(rx, Duration::from_millis(300));
    std::thread::sleep(Duration::from_millis(700)); // logger flush
    (rep, log_path, window)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let port = args.next().unwrap_or_else(|| "COM9".into());
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut bauds: Vec<u32> = args.filter_map(|a| a.parse().ok()).collect();
    if bauds.is_empty() {
        bauds = vec![115_200, 250_000, 500_000, 1_000_000, 2_000_000];
    }

    let (tx, rx) = mpsc::channel();
    let mgr = Arc::new(SerialManager::new(tx));
    let mut board_baud = BOARD_DEFAULT_BAUD;

    println!("{:>9} | {:>8} | {:>10} | {:>9} | {:>5} | {:>4} | {:>5} | {:>10} | log", "baud", "lines/s", "bytes/s", "% of line", "lost", "bad", "malf", "app µs/ln");
    println!("{}", "-".repeat(96));
    for baud in bauds {
        if baud != board_baud {
            switch_baud(&mgr, &rx, &port, board_baud, baud);
            board_baud = baud;
        }
        let (rep, log_path, elapsed) = run(&mgr, &rx, &port, baud, secs);

        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        let log_data_lines = log.lines().filter(|l| l.contains("] RX $")).count() as u64;
        let _ = std::fs::remove_file(&log_path);

        let bps = rep.bytes as f64 / elapsed;
        let line_max = baud as f64 / 10.0; // 8N1: 10 bits per byte
        let per_line_us = rep.process_time.as_secs_f64() * 1e6 / rep.lines.max(1) as f64;
        println!(
            "{baud:>9} | {:>8.0} | {:>10.0} | {:>8.1}% | {:>5} | {:>4} | {:>5} | {:>10.2} | {}",
            bps / 56.0,
            bps,
            100.0 * bps / line_max,
            rep.lost,
            rep.bad_checksum,
            rep.malformed,
            per_line_us,
            if log_data_lines == rep.lines { format!("complete ({log_data_lines})") } else { format!("MISMATCH log={log_data_lines} rx={}", rep.lines) },
        );
        for i in rep.info.iter().take(3) {
            println!("          info: {i}");
        }
    }

    // Leave the board at its power-on default.
    if board_baud != BOARD_DEFAULT_BAUD {
        switch_baud(&mgr, &rx, &port, board_baud, BOARD_DEFAULT_BAUD);
    }
    open(&mgr, &port, BOARD_DEFAULT_BAUD);
    mgr.write("hs", b"G\n", false).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    mgr.disconnect("hs");
    mgr.shutdown();
}
