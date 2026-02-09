use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
#[derive(Debug)]
pub struct PerformanceMetrics {

    pub reads: AtomicU64,
    pub writes: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,


    pub read_latencies: Arc<RwLock<LatencyTracker>>,
    pub write_latencies: Arc<RwLock<LatencyTracker>>,


    pub operations_per_second: AtomicU64,
    pub bytes_read: AtomicU64,
    pub bytes_written: AtomicU64,


    pub errors: AtomicU64,
    pub timeouts: AtomicU64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            reads: AtomicU64::new(0),
            writes: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            read_latencies: Arc::new(RwLock::new(LatencyTracker::new())),
            write_latencies: Arc::new(RwLock::new(LatencyTracker::new())),
            operations_per_second: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            timeouts: AtomicU64::new(0),
        }
    }
}

impl PerformanceMetrics {
    pub fn record_read(&self, latency: Duration, bytes: usize, cache_hit: bool) {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.bytes_read.fetch_add(bytes as u64, Ordering::Relaxed);

        if cache_hit {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cache_misses.fetch_add(1, Ordering::Relaxed);
        }

        let latencies = self.read_latencies.clone();
        tokio::spawn(async move {
            let mut tracker = latencies.write().await;
            tracker.record(latency);
        });
    }

    pub fn record_write(&self, latency: Duration, bytes: usize) {
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.bytes_written
            .fetch_add(bytes as u64, Ordering::Relaxed);

        let latencies = self.write_latencies.clone();
        tokio::spawn(async move {
            let mut tracker = latencies.write().await;
            tracker.record(latency);
        });
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_timeout(&self) {
        self.timeouts.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn get_summary(&self) -> PerformanceSummary {
        let read_stats = self.read_latencies.read().await.get_stats();
        let write_stats = self.write_latencies.read().await.get_stats();

        PerformanceSummary {
            total_reads: self.reads.load(Ordering::Relaxed),
            total_writes: self.writes.load(Ordering::Relaxed),
            cache_hit_rate: {
                let hits = self.cache_hits.load(Ordering::Relaxed);
                let misses = self.cache_misses.load(Ordering::Relaxed);
                if hits + misses > 0 {
                    hits as f64 / (hits + misses) as f64
                } else {
                    0.0
                }
            },
            read_latency: read_stats,
            write_latency: write_stats,
            bytes_read: self.bytes_read.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            total_errors: self.errors.load(Ordering::Relaxed),
            total_timeouts: self.timeouts.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PerformanceSummary {
    pub total_reads: u64,
    pub total_writes: u64,
    pub cache_hit_rate: f64,
    pub read_latency: LatencyStats,
    pub write_latency: LatencyStats,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub total_errors: u64,
    pub total_timeouts: u64,
}


#[derive(Debug)]
pub struct LatencyTracker {
    samples: VecDeque<Duration>,
    max_samples: usize,
}

impl LatencyTracker {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(1000),
            max_samples: 1000,
        }
    }

    pub fn record(&mut self, latency: Duration) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(latency);
    }

    pub fn get_stats(&self) -> LatencyStats {
        if self.samples.is_empty() {
            return LatencyStats::default();
        }

        let mut sorted: Vec<Duration> = self.samples.iter().cloned().collect();
        sorted.sort();

        let len = sorted.len();
        let sum: Duration = sorted.iter().sum();

        LatencyStats {
            count: len as u64,
            min: sorted[0],
            max: sorted[len - 1],
            avg: sum / len as u32,
            p50: sorted[len * 50 / 100],
            p95: sorted[len * 95 / 100],
            p99: sorted[len * 99 / 100],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LatencyStats {
    pub count: u64,
    pub min: Duration,
    pub max: Duration,
    pub avg: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
}


pub struct AdaptiveCacheManager {
    current_size: AtomicUsize,
    min_size: usize,
    max_size: usize,
    target_hit_rate: f64,
    adjustment_interval: Duration,
    last_adjustment: Arc<RwLock<Instant>>,
}

impl AdaptiveCacheManager {
    pub fn new(
        initial_size: usize,
        min_size: usize,
        max_size: usize,
        target_hit_rate: f64,
    ) -> Self {
        Self {
            current_size: AtomicUsize::new(initial_size),
            min_size,
            max_size,
            target_hit_rate,
            adjustment_interval: Duration::from_secs(30),
            last_adjustment: Arc::new(RwLock::new(Instant::now())),
        }
    }

    pub async fn adjust_size(&self, metrics: &PerformanceMetrics) -> Option<usize> {
        let now = Instant::now();
        let mut last_adj = self.last_adjustment.write().await;

        if now.duration_since(*last_adj) < self.adjustment_interval {
            return None;
        }

        *last_adj = now;
        drop(last_adj);

        let summary = metrics.get_summary().await;
        let current_size = self.current_size.load(Ordering::Relaxed);

        let new_size = if summary.cache_hit_rate < self.target_hit_rate {

            (current_size as f64 * 1.2).min(self.max_size as f64) as usize
        } else if summary.cache_hit_rate > self.target_hit_rate + 0.1 {

            (current_size as f64 * 0.9).max(self.min_size as f64) as usize
        } else {
            current_size
        };

        if new_size != current_size {
            self.current_size.store(new_size, Ordering::Relaxed);
            Some(new_size)
        } else {
            None
        }
    }

    pub fn get_current_size(&self) -> usize {
        self.current_size.load(Ordering::Relaxed)
    }
}


pub struct BatchOptimizer {
    pending_writes: Arc<RwLock<Vec<(String, Vec<u8>)>>>,
    batch_size: usize,
    batch_timeout: Duration,
    last_flush: Arc<RwLock<Instant>>,
}

impl BatchOptimizer {
    pub fn new(batch_size: usize, batch_timeout: Duration) -> Self {
        Self {
            pending_writes: Arc::new(RwLock::new(Vec::with_capacity(batch_size))),
            batch_size,
            batch_timeout,
            last_flush: Arc::new(RwLock::new(Instant::now())),
        }
    }

    pub async fn add_write(&self, key: String, value: Vec<u8>) -> Option<Vec<(String, Vec<u8>)>> {
        let mut pending = self.pending_writes.write().await;
        pending.push((key, value));

        if pending.len() >= self.batch_size {
            let batch = pending.drain(..).collect();
            *self.last_flush.write().await = Instant::now();
            Some(batch)
        } else {
            None
        }
    }

    pub async fn flush_if_needed(&self) -> Option<Vec<(String, Vec<u8>)>> {
        let now = Instant::now();
        let last_flush = *self.last_flush.read().await;

        if now.duration_since(last_flush) >= self.batch_timeout {
            let mut pending = self.pending_writes.write().await;
            if !pending.is_empty() {
                let batch = pending.drain(..).collect();
                *self.last_flush.write().await = now;
                Some(batch)
            } else {
                None
            }
        } else {
            None
        }
    }
}


pub struct LoadBalancer {
    servers: Vec<String>,
    current_index: AtomicUsize,
    health_status: Arc<RwLock<Vec<bool>>>,
}

impl LoadBalancer {
    pub fn new(servers: Vec<String>) -> Self {
        let health_status = vec![true; servers.len()];
        Self {
            servers,
            current_index: AtomicUsize::new(0),
            health_status: Arc::new(RwLock::new(health_status)),
        }
    }

    pub async fn get_next_server(&self) -> Option<String> {
        let health = self.health_status.read().await;
        let healthy_servers: Vec<(usize, &String)> = self
            .servers
            .iter()
            .enumerate()
            .filter(|(i, _)| health[*i])
            .collect();

        if healthy_servers.is_empty() {
            return None;
        }

        let current = self.current_index.fetch_add(1, Ordering::Relaxed);
        let index = current % healthy_servers.len();
        Some(healthy_servers[index].1.clone())
    }

    pub async fn mark_server_unhealthy(&self, server: &str) {
        if let Some(index) = self.servers.iter().position(|s| s == server) {
            let mut health = self.health_status.write().await;
            health[index] = false;
        }
    }

    pub async fn mark_server_healthy(&self, server: &str) {
        if let Some(index) = self.servers.iter().position(|s| s == server) {
            let mut health = self.health_status.write().await;
            health[index] = true;
        }
    }
}
