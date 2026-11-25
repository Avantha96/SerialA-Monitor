//! Asynchronous buffered disk logger.
//!
//! Serial threads hand complete batches to one logging thread over a channel,
//! so disk I/O never sits on the read path.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

const BUF_CAPACITY: usize = 256 * 1024;
const FLUSH_EVERY: Duration = Duration::from_millis(500);

pub enum LogMsg {
    Start {
        session: String,
        path: PathBuf,
    },
    Batch {
        session: String,
        timestamp: String,
        blob: Arc<Vec<u8>>,
        lens: Vec<u32>,
        tag: &'static str,
        hex: bool,
    },
    Stop(String),
    CloseAll,
}

pub struct Logger {
    tx: Mutex<Sender<LogMsg>>,
}

/// Make a port name like `COM3` or `/dev/ttyUSB0` safe for a file name.
fn sanitize(label: &str) -> String {
    let s: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "session".into()
    } else {
        s
    }
}

impl Logger {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<LogMsg>();
        std::thread::Builder::new()
            .name("serialx-native-logger".into())
            .spawn(move || run(rx))
            .expect("spawn logger thread");
        Self { tx: Mutex::new(tx) }
    }

    /// Open `dir/<label>_<timestamp>.log` for `session`. Returns the path.
    pub fn start(&self, dir: PathBuf, session: &str, label: &str) -> Result<String, String> {
        fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

        let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let path = dir.join(format!("{}_{stamp}.log", sanitize(label)));
        // Open once here so permission errors are reported to the user, not swallowed.
        File::options()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let display = path.to_string_lossy().to_string();

        self.tx
            .lock()
            .send(LogMsg::Start {
                session: session.to_string(),
                path,
            })
            .map_err(|_| "logger thread is gone".to_string())?;
        Ok(display)
    }

    pub fn stop(&self, session: &str) {
        let _ = self.tx.lock().send(LogMsg::Stop(session.to_string()));
    }

    pub fn batch(
        &self,
        session: &str,
        timestamp: String,
        blob: Arc<Vec<u8>>,
        lens: Vec<u32>,
        tag: &'static str,
        hex: bool,
    ) {
        let _ = self.tx.lock().send(LogMsg::Batch {
            session: session.to_string(),
            timestamp,
            blob,
            lens,
            tag,
            hex,
        });
    }

    pub fn close_all(&self) {
        let _ = self.tx.lock().send(LogMsg::CloseAll);
    }
}

fn write_hex(out: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut scratch = Vec::with_capacity(bytes.len() * 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            scratch.push(b' ');
        }
        scratch.push(DIGITS[(b >> 4) as usize]);
        scratch.push(DIGITS[(b & 0x0f) as usize]);
    }
    out.write_all(&scratch)
}

fn run(rx: Receiver<LogMsg>) {
    let mut files: HashMap<String, BufWriter<File>> = HashMap::new();

    loop {
        match rx.recv_timeout(FLUSH_EVERY) {
            Ok(LogMsg::Start { session, path }) => {
                if let Some(mut old) = files.remove(&session) {
                    let _ = old.flush();
                }
                match File::options().create(true).append(true).open(&path) {
                    Ok(f) => {
                        files.insert(session, BufWriter::with_capacity(BUF_CAPACITY, f));
                    }
                    Err(e) => eprintln!("log open failed for {}: {e}", path.display()),
                }
            }
            Ok(LogMsg::Batch {
                session,
                timestamp,
                blob,
                lens,
                tag,
                hex,
            }) => {
                let Some(file) = files.get_mut(&session) else {
                    continue;
                };
                let mut off = 0usize;
                for len in lens {
                    let end = (off + len as usize).min(blob.len());
                    let mut line = &blob[off..end];
                    off = end;
                    if !hex {
                        // TX text carries its own line ending; one newline per entry is enough.
                        while let [rest @ .., b'\r' | b'\n'] = line {
                            line = rest;
                        }
                    }

                    let _ = write!(file, "[{timestamp}] {tag} ");
                    let r = if hex { write_hex(file, line) } else { file.write_all(line) };
                    if r.is_err() {
                        break;
                    }
                    let _ = file.write_all(b"\n");
                }
            }
            Ok(LogMsg::Stop(session)) => {
                if let Some(mut f) = files.remove(&session) {
                    let _ = f.flush();
                }
            }
            Ok(LogMsg::CloseAll) => {
                for (_, mut f) in files.drain() {
                    let _ = f.flush();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                for f in files.values_mut() {
                    let _ = f.flush();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                for (_, mut f) in files.drain() {
                    let _ = f.flush();
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_tagged_lines() {
        let dir = std::env::temp_dir().join(format!("serialx_log_test_{}", std::process::id()));
        let logger = Logger::spawn();
        let path = logger.start(dir.clone(), "s1", "COM3").unwrap();
        logger.batch("s1", "T".into(), Arc::new(b"helloAB".to_vec()), vec![5, 2], "RX", false);
        logger.batch("s1", "T".into(), Arc::new(vec![0x01, 0xFF]), vec![2], "TX", true);
        logger.stop("s1");
        // Give the logger thread a moment to flush.
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(10));
            if let Ok(s) = fs::read_to_string(&path) {
                if s.lines().count() == 3 {
                    assert_eq!(s, "[T] RX hello\n[T] RX AB\n[T] TX 01 FF\n");
                    let _ = fs::remove_dir_all(&dir);
                    return;
                }
            }
        }
        panic!("log file not written");
    }

    #[test]
    fn sanitizes_port_names() {
        assert_eq!(sanitize("/dev/ttyUSB0"), "dev_ttyUSB0");
        assert_eq!(sanitize("COM3"), "COM3");
        assert_eq!(sanitize("///"), "session");
    }
}
