//! Monitoring module - structured logging and metrics
//!
//! Provides:
//! - Structured logging utilities
//! - Metrics collection for Prometheus export
//! - Health check utilities

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Standard Prometheus histogram bucket boundaries
pub const HISTOGRAM_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Initialize structured logging with default settings
pub fn init_logging() {
    // Logging is configured via tracing-subscriber in the application
    // This is a placeholder that can be called during startup
    tracing::info!("Logging initialized");
}

/// Metrics collector for Prometheus export
#[derive(Clone)]
pub struct MetricsCollector {
    counters: Arc<RwLock<HashMap<String, u64>>>,
    gauges: Arc<RwLock<HashMap<String, f64>>>,
    histograms: Arc<RwLock<HashMap<String, Vec<f64>>>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            counters: Arc::new(RwLock::new(HashMap::new())),
            gauges: Arc::new(RwLock::new(HashMap::new())),
            histograms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Increment a counter
    pub fn inc_counter(&self, name: &str, value: u64) {
        if let Ok(mut counters) = self.counters.write() {
            *counters.entry(name.to_string()).or_insert(0) += value;
        }
    }

    /// Set a gauge value
    pub fn set_gauge(&self, name: &str, value: f64) {
        if let Ok(mut gauges) = self.gauges.write() {
            gauges.insert(name.to_string(), value);
        }
    }

    /// Observe a value for a histogram metric
    pub fn observe_histogram(&self, name: &str, value: f64) {
        if let Ok(mut histograms) = self.histograms.write() {
            histograms
                .entry(name.to_string())
                .or_insert_with(Vec::new)
                .push(value);
        }
    }

    /// Record a duration as seconds into a histogram
    pub fn record_duration(&self, name: &str, duration: Duration) {
        self.observe_histogram(name, duration.as_secs_f64());
    }

    /// Generate Prometheus metrics text
    pub fn to_prometheus_text(&self) -> String {
        let mut output = String::new();

        if let Ok(counters) = self.counters.read() {
            for (name, value) in counters.iter() {
                let safe_name = name.replace('-', "_");
                output.push_str(&format!(
                    "# TYPE {} counter\n{} {}\n",
                    safe_name, safe_name, value
                ));
            }
        }

        if let Ok(gauges) = self.gauges.read() {
            for (name, value) in gauges.iter() {
                let safe_name = name.replace('-', "_");
                output.push_str(&format!(
                    "# TYPE {} gauge\n{} {}\n",
                    safe_name, safe_name, value
                ));
            }
        }

        if let Ok(histograms) = self.histograms.read() {
            for (name, values) in histograms.iter() {
                let safe_name = name.replace('-', "_");
                let count = values.len();
                let sum: f64 = values.iter().sum();

                output.push_str(&format!("# TYPE {} histogram\n", safe_name));

                for bucket in HISTOGRAM_BUCKETS {
                    let bucket_count = values.iter().filter(|&&v| v <= *bucket).count();
                    output.push_str(&format!(
                        "{}_bucket{{le=\"{}\"}} {}\n",
                        safe_name, bucket, bucket_count
                    ));
                }
                // +Inf bucket always equals total count
                output.push_str(&format!("{}_bucket{{le=\"+Inf\"}} {}\n", safe_name, count));
                output.push_str(&format!("{}_sum {}\n", safe_name, sum));
                output.push_str(&format!("{}_count {}\n", safe_name, count));
            }
        }

        output
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static::lazy_static! {
    pub static ref METRICS: MetricsCollector = MetricsCollector::new();
}

/// Initialize metrics collector
pub fn init_metrics() {
    tracing::info!("Metrics collector initialized");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_increment() {
        let collector = MetricsCollector::new();
        collector.inc_counter("test_counter", 1);
        collector.inc_counter("test_counter", 2);

        let text = collector.to_prometheus_text();
        assert!(text.contains("test_counter 3"));
    }

    #[test]
    fn test_gauge_set() {
        let collector = MetricsCollector::new();
        collector.set_gauge("test_gauge", 42.0);

        let text = collector.to_prometheus_text();
        assert!(text.contains("test_gauge 42"));
    }

    #[test]
    fn test_observe_histogram() {
        let collector = MetricsCollector::new();
        collector.observe_histogram("request_duration", 0.003);
        collector.observe_histogram("request_duration", 0.07);
        collector.observe_histogram("request_duration", 0.5);
        collector.observe_histogram("request_duration", 3.0);

        let text = collector.to_prometheus_text();

        assert!(text.contains("# TYPE request_duration histogram"));
        // 0.003 <= 0.005, so bucket le=0.005 should be 1
        assert!(text.contains("request_duration_bucket{le=\"0.005\"} 1"));
        // 0.003, 0.07 <= 0.1, so bucket le=0.1 should be 2
        assert!(text.contains("request_duration_bucket{le=\"0.1\"} 2"));
        // 0.003, 0.07, 0.5 <= 0.5, so bucket le=0.5 should be 3
        assert!(text.contains("request_duration_bucket{le=\"0.5\"} 3"));
        // 0.003, 0.07, 0.5, 3.0 <= 5.0, so bucket le=5 should be 4
        assert!(text.contains("request_duration_bucket{le=\"5\"} 4"));
        // +Inf always equals count
        assert!(text.contains("request_duration_bucket{le=\"+Inf\"} 4"));
        assert!(text.contains("request_duration_count 4"));
        // sum = 0.003 + 0.07 + 0.5 + 3.0 = 3.573
        assert!(text.contains("request_duration_sum 3.573"));
    }

    #[test]
    fn test_record_duration() {
        let collector = MetricsCollector::new();
        collector.record_duration("handler_time", Duration::from_millis(150));
        collector.record_duration("handler_time", Duration::from_secs(2));

        let text = collector.to_prometheus_text();

        assert!(text.contains("# TYPE handler_time histogram"));
        assert!(text.contains("handler_time_bucket{le=\"+Inf\"} 2"));
        assert!(text.contains("handler_time_count 2"));
        // 0.15 <= 0.25, so bucket le=0.25 should be 1
        assert!(text.contains("handler_time_bucket{le=\"0.25\"} 1"));
        // both <= 2.5
        assert!(text.contains("handler_time_bucket{le=\"2.5\"} 2"));
    }

    #[test]
    fn test_histogram_empty() {
        let collector = MetricsCollector::new();
        let text = collector.to_prometheus_text();
        // No histogram output when none observed
        assert!(!text.contains("histogram"));
    }

    #[test]
    fn test_histogram_buckets_const() {
        assert_eq!(HISTOGRAM_BUCKETS.len(), 11);
        assert_eq!(HISTOGRAM_BUCKETS[0], 0.005);
        assert_eq!(HISTOGRAM_BUCKETS[10], 10.0);
        // Verify sorted
        for i in 1..HISTOGRAM_BUCKETS.len() {
            assert!(HISTOGRAM_BUCKETS[i] > HISTOGRAM_BUCKETS[i - 1]);
        }
    }
}
