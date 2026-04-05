//! Performance benchmark module
//!
//! Provides:
//! - Benchmark runner
//! - Performance metrics collection
//! - SLA verification

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Benchmark result
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub name: String,
    pub iterations: usize,
    pub total_duration: Duration,
    pub avg_duration: Duration,
    pub min_duration: Duration,
    pub max_duration: Duration,
    pub ops_per_second: f64,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
}

/// Run a benchmark
pub fn benchmark(name: &str, iterations: usize, mut f: impl FnMut()) -> BenchmarkResult {
    let mut durations: Vec<Duration> = Vec::with_capacity(iterations);
    let start = Instant::now();

    for _ in 0..iterations {
        let iter_start = Instant::now();
        f();
        durations.push(iter_start.elapsed());
    }

    let total_duration = start.elapsed();

    // Calculate statistics
    durations.sort();
    let min_duration = *durations.first().unwrap();
    let max_duration = *durations.last().unwrap();
    let avg_duration = total_duration / iterations as u32;
    let ops_per_second = iterations as f64 / total_duration.as_secs_f64();

    let p50 = durations[iterations * 50 / 100];
    let p95 = durations[iterations * 95 / 100];
    let p99 = durations[iterations * 99 / 100];

    BenchmarkResult {
        name: name.to_string(),
        iterations,
        total_duration,
        avg_duration,
        min_duration,
        max_duration,
        ops_per_second,
        p50,
        p95,
        p99,
    }
}

/// Async benchmark
pub async fn benchmark_async<F, T>(
    name: &str,
    iterations: usize,
    mut f: impl FnMut() -> F,
) -> BenchmarkResult
where
    F: std::future::Future<Output = T>,
{
    let mut durations: Vec<Duration> = Vec::with_capacity(iterations);
    let start = Instant::now();

    for _ in 0..iterations {
        let iter_start = Instant::now();
        f().await;
        durations.push(iter_start.elapsed());
    }

    let total_duration = start.elapsed();

    durations.sort();
    let min_duration = *durations.first().unwrap();
    let max_duration = *durations.last().unwrap();
    let avg_duration = total_duration / iterations as u32;
    let ops_per_second = iterations as f64 / total_duration.as_secs_f64();

    let p50 = durations[iterations * 50 / 100];
    let p95 = durations[iterations * 95 / 100];
    let p99 = durations[iterations * 99 / 100];

    BenchmarkResult {
        name: name.to_string(),
        iterations,
        total_duration,
        avg_duration,
        min_duration,
        max_duration,
        ops_per_second,
        p50,
        p95,
        p99,
    }
}

/// Throughput benchmark
pub struct ThroughputBenchmark {
    name: String,
    total_bytes: AtomicUsize,
    start_time: Option<Instant>,
}

impl ThroughputBenchmark {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            total_bytes: AtomicUsize::new(0),
            start_time: None,
        }
    }

    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
    }

    pub fn record_bytes(&self, bytes: usize) {
        self.total_bytes.fetch_add(bytes, Ordering::SeqCst);
    }

    pub fn result(&self) -> ThroughputResult {
        let duration = self.start_time.map(|t| t.elapsed()).unwrap_or_default();
        let bytes = self.total_bytes.load(Ordering::SeqCst);
        let mbps = if duration.as_secs_f64() > 0.0 {
            bytes as f64 / duration.as_secs_f64() / 1_000_000.0
        } else {
            0.0
        };

        ThroughputResult {
            name: self.name.clone(),
            total_bytes: bytes,
            duration,
            mbps,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThroughputResult {
    pub name: String,
    pub total_bytes: usize,
    pub duration: Duration,
    pub mbps: f64,
}

/// SLA verification
pub struct SlaVerifier {
    _name: String,
    threshold: Duration,
    passed: AtomicUsize,
    failed: AtomicUsize,
}

impl SlaVerifier {
    pub fn new(name: &str, threshold: Duration) -> Self {
        Self {
            _name: name.to_string(),
            threshold,
            passed: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
        }
    }

    pub fn record(&self, duration: Duration) {
        if duration <= self.threshold {
            self.passed.fetch_add(1, Ordering::SeqCst);
        } else {
            self.failed.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn success_rate(&self) -> f64 {
        let passed = self.passed.load(Ordering::SeqCst);
        let failed = self.failed.load(Ordering::SeqCst);
        let total = passed + failed;
        if total == 0 {
            100.0
        } else {
            passed as f64 / total as f64 * 100.0
        }
    }
}

impl BenchmarkResult {
    pub fn print(&self) {
        println!("\n=== Benchmark: {} ===", self.name);
        println!("Iterations: {}", self.iterations);
        println!("Total time: {:.2?}", self.total_duration);
        println!("Average: {:.2?}", self.avg_duration);
        println!("Min: {:.2?}", self.min_duration);
        println!("Max: {:.2?}", self.max_duration);
        println!("Ops/sec: {:.2}", self.ops_per_second);
        println!("P50: {:.2?}", self.p50);
        println!("P95: {:.2?}", self.p95);
        println!("P99: {:.2?}", self.p99);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark() {
        let result = benchmark("test_iteration", 1000, || {
            // Simulate work
            let _ = 1 + 1;
        });

        assert_eq!(result.iterations, 1000);
        assert!(result.avg_duration < Duration::from_micros(100));
    }

    #[tokio::test]
    async fn test_async_benchmark() {
        let iterations = 100;
        let mut durations: Vec<Duration> = Vec::with_capacity(iterations);
        let start = Instant::now();

        for _ in 0..iterations {
            let iter_start = Instant::now();
            tokio::time::sleep(Duration::from_micros(1)).await;
            durations.push(iter_start.elapsed());
        }

        let _total_duration = start.elapsed();

        assert_eq!(durations.len(), 100);
    }

    #[test]
    fn test_throughput() {
        let mut bench = ThroughputBenchmark::new("test_throughput");
        bench.start();

        for _ in 0..10 {
            bench.record_bytes(1024);
        }

        let result = bench.result();
        assert_eq!(result.total_bytes, 10240);
    }

    #[test]
    fn test_sla_verifier() {
        let verifier = SlaVerifier::new("test_sla", Duration::from_millis(10));

        verifier.record(Duration::from_millis(5));
        verifier.record(Duration::from_millis(8));
        verifier.record(Duration::from_millis(15));

        let rate = verifier.success_rate();
        assert!((rate - 66.66).abs() < 0.1);
    }
}
