//! Hardware probe: exercises the real SerialManager against a connected device.
//!
//!   cargo run --example hw_probe -- COM9 [baud ...]
//!
//! For each baud rate: open without touching DTR (no reset) and listen, then
//! re-open with DTR asserted (Arduino auto-reset) and listen again. Also checks
//! write, disk logging, stats and clean disconnect.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use seriala_native::serial::{SerialConfig, SerialEvent, SerialManager};

fn listen(rx: &mpsc::Receiver<SerialEvent>, secs: u64) -> Vec<String> {
    let mut out = Vec::new();
    let end = Instant::now() + Duration::from_secs(secs);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            // Only complete lines; provisional tails are what the UI shows while waiting.
            Ok(SerialEvent::Data { lines, .. }) => {
                for l in lines {
                    out.push(String::from_utf8_lossy(&l).to_string());
                }
            }
            Ok(SerialEvent::Error { error, .. }) => out.push(format!("<error: {error}>")),
            Ok(SerialEvent::Closed { deliberate, .. }) => out.push(format!("<closed deliberate={deliberate}>")),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    out
}

fn show(label: &str, lines: &[String]) {
    println!("--- {label}: {} line(s)", lines.len());
    for l in lines.iter().take(15) {
        println!("    | {l}");
    }
    if lines.len() > 15 {
        println!("    | … {} more", lines.len() - 15);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let port = args.next().unwrap_or_else(|| "COM9".into());
    let bauds: Vec<u32> = {
        let v: Vec<u32> = args.filter_map(|a| a.parse().ok()).collect();
        if v.is_empty() { vec![9600, 115200] } else { v }
    };

    println!("ports: {:?}", SerialManager::list_ports());
    let (tx, rx) = mpsc::channel();
    let mgr = Arc::new(SerialManager::new(tx));
    let id = "probe";

    for baud in bauds {
        for dtr in [false, true] {
            let cfg = SerialConfig { path: port.clone(), baud_rate: baud, dtr_rts: dtr, ..Default::default() };
            let t0 = Instant::now();
            if let Err(e) = mgr.connect(id, cfg) {
                println!("open {port} @ {baud} (dtr={dtr}) FAILED: {e}");
                continue;
            }
            println!("open {port} @ {baud} dtr={dtr} in {:?}", t0.elapsed());
            let lines = listen(&rx, if dtr { 4 } else { 3 });
            show(&format!("{baud} baud, dtr={dtr}"), &lines);
            if let Some(s) = mgr.stats(id) {
                println!("    stats: rx={}B reads={} rate={}B/s", s.total_rx, s.rx_chunks, s.rx_rate);
            }
            mgr.disconnect(id);
            let tail = listen(&rx, 1);
            println!("    after disconnect: {tail:?}");
        }
    }

    // Write + logging round trip at the first baud.
    let cfg = SerialConfig { path: port.clone(), baud_rate: 9600, ..Default::default() };
    mgr.connect(id, cfg).expect("reopen");
    let path = mgr.set_logging(id, true, &port).expect("logging").expect("path");
    if std::env::var_os("PROBE_LOG_HEX").is_some() {
        mgr.set_log_hex(id, true);
    }
    mgr.write(id, b"hello from SerialA\n", false).expect("write");
    let lines = listen(&rx, 2);
    show("after write (echo sketches reply here)", &lines);
    let _ = mgr.set_logging(id, false, "");
    mgr.disconnect(id);
    std::thread::sleep(Duration::from_millis(800));
    let log = std::fs::read_to_string(&path).unwrap_or_default();
    println!("log file {path}:\n{}", log.lines().take(10).collect::<Vec<_>>().join("\n"));
    let _ = std::fs::remove_file(&path);
    mgr.shutdown();
}
