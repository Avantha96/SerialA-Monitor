//! Per-session traffic counters.
//!
//! Everything the Statistics panel used to derive by re-scanning the 5000-message
//! ring buffer on every render is accumulated here instead: one mutex acquisition
//! per read chunk (not per byte), so the UI just reads a snapshot.

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

const RATE_BUCKET: Duration = Duration::from_secs(1);

struct Inner {
    total_rx: u64,
    total_tx: u64,
    rx_chunks: u64,
    tx_chunks: u64,
    /// Byte-value histogram over received data.
    hist: [u64; 256],
    /// Bytes accumulated in the bucket that started at `bucket_start`.
    bucket_start: Instant,
    bucket_rx: u64,
    bucket_tx: u64,
    /// Last *completed* one-second bucket — this is what we report as B/s.
    last_rx_rate: u64,
    last_tx_rate: u64,
}

pub struct Stats {
    inner: Mutex<Inner>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatsSnapshot {
    pub total_rx: u64,
    pub total_tx: u64,
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub rx_chunks: u64,
    pub tx_chunks: u64,
    /// Top byte values by frequency, most frequent first.
    pub top_bytes: Vec<ByteCount>,
}

#[derive(Serialize, Clone)]
pub struct ByteCount {
    pub byte: u8,
    pub count: u64,
}

impl Stats {
    pub fn new(now: Instant) -> Self {
        Self {
            inner: Mutex::new(Inner {
                total_rx: 0,
                total_tx: 0,
                rx_chunks: 0,
                tx_chunks: 0,
                hist: [0; 256],
                bucket_start: now,
                bucket_rx: 0,
                bucket_tx: 0,
                last_rx_rate: 0,
                last_tx_rate: 0,
            }),
        }
    }

    /// Roll the rate bucket forward if it has expired. Caller holds the lock.
    fn roll(inner: &mut Inner) {
        let elapsed = inner.bucket_start.elapsed();
        if elapsed < RATE_BUCKET {
            return;
        }
        // More than one bucket may have elapsed; anything older than the most
        // recent full bucket reports as zero, which is the honest answer.
        if elapsed < RATE_BUCKET * 2 {
            inner.last_rx_rate = inner.bucket_rx;
            inner.last_tx_rate = inner.bucket_tx;
        } else {
            inner.last_rx_rate = 0;
            inner.last_tx_rate = 0;
        }
        inner.bucket_rx = 0;
        inner.bucket_tx = 0;
        inner.bucket_start = Instant::now();
    }

    pub fn record_rx(&self, chunk: &[u8]) {
        let mut inner = self.inner.lock();
        Self::roll(&mut inner);
        inner.total_rx += chunk.len() as u64;
        inner.bucket_rx += chunk.len() as u64;
        inner.rx_chunks += 1;
        for &b in chunk {
            inner.hist[b as usize] += 1;
        }
    }

    pub fn record_tx(&self, len: usize) {
        let mut inner = self.inner.lock();
        Self::roll(&mut inner);
        inner.total_tx += len as u64;
        inner.bucket_tx += len as u64;
        inner.tx_chunks += 1;
    }

    pub fn snapshot(&self, top_n: usize) -> StatsSnapshot {
        let mut inner = self.inner.lock();
        Self::roll(&mut inner);

        let mut top: Vec<ByteCount> = inner
            .hist
            .iter()
            .enumerate()
            .filter(|(_, &c)| c > 0)
            .map(|(b, &count)| ByteCount { byte: b as u8, count })
            .collect();
        top.sort_unstable_by(|a, b| b.count.cmp(&a.count).then(a.byte.cmp(&b.byte)));
        top.truncate(top_n);

        StatsSnapshot {
            total_rx: inner.total_rx,
            total_tx: inner.total_tx,
            rx_rate: inner.last_rx_rate,
            tx_rate: inner.last_tx_rate,
            rx_chunks: inner.rx_chunks,
            tx_chunks: inner.tx_chunks,
            top_bytes: top,
        }
    }
}
