//! Disk logging on a dedicated thread.
//!
//! The reader thread never touches the filesystem: it hands a whole batch
//! (one Arc'd byte blob + line lengths) to this thread, which formats and
//! writes through a 256 KiB BufWriter and flushes on a timer. A slow or
//! sleeping disk therefore cannot stall serial reads.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use parking_lot::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

const BUF_CAPACITY: usize = 256 * 1024;
const FLUSH_EVERY: Duration = Duration::from_millis(500);

pub enum LogMsg {
    Start {
        session: String,
        path: PathBuf,
    },
    /// A batch of lines: `blob` is the concatenation, `lens` slices it up.
    Batch {
        session: String,
        timestamp: String,
        blob: Arc<Vec<u8>>,
        lens: Vec<u32>,
        hex: bool,
    },
    Stop(String),
    CloseAll,
}

/// `mpsc::Sender` is `!Sync`, so it lives behind a mutex - `SerialManager`
/// has to be `Sync` to be Tauri managed state. Contention is negligible:
/// the lock is only taken by sessions that actually have logging on.
pub struct Logger {
    tx: Mutex<Sender<LogMsg>>,
}

impl Logger {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<LogMsg>();
        std::thread::Builder::new()
            .name("serialx-logger".into())
            .spawn(move || run(rx))
            .expect("spawn logger thread");
        Self { tx: Mutex::new(tx) }
    }

    /// Creates the log directory and returns the file path that will be used.
    /// The file itself is opened on the logger thread.
    pub fn start(&self, docs: PathBuf, session: &str) -> Result<String, String> {
        let dir = docs.join("SerialMonitorLogs");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let stamp = chrono::Utc::now()
            .format("%Y-%m-%dT%H-%M-%S-%3fZ")
            .to_string();
        let path = dir.join(format!("session_{session}_{stamp}.txt"));
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
        hex: bool,
    ) {
        let _ = self.tx.lock().send(LogMsg::Batch {
            session: session.to_string(),
            timestamp,
            blob,
            lens,
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
        // Wake at least every FLUSH_EVERY so buffered data reaches disk even
        // when a port has gone quiet.
        match rx.recv_timeout(FLUSH_EVERY) {
            Ok(LogMsg::Start { session, path }) => {
                if files.contains_key(&session) {
                    continue;
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
                hex,
            }) => {
                let Some(file) = files.get_mut(&session) else {
                    continue;
                };
                let mut off = 0usize;
                for len in lens {
                    let end = off + len as usize;
                    let line = &blob[off..end.min(blob.len())];
                    off = end;

                    let _ = file.write_all(b"[");
                    let _ = file.write_all(timestamp.as_bytes());
                    let _ = file.write_all(if hex { b"] [HEX] " } else { b"] [ASC] " });
                    let r = if hex {
                        write_hex(file, line)
                    } else {
                        file.write_all(line)
                    };
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
