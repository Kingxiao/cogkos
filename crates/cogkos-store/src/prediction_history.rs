//! Prediction History Store
//!
//! Stores prediction error history for analysis and evolution engine feedback.
//! Uses In-memory storage for development/testing.

use async_trait::async_trait;
use cogkos_core::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Record for storing prediction error history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionErrorRecord {
    /// Unique identifier for this record
    pub record_id: String,
    /// Tenant ID for multi-tenancy
    pub tenant_id: String,
    /// Claim ID associated with this prediction
    pub claim_id: String,
    /// Prediction validation ID
    pub validation_id: String,
    /// Predicted probability (0-1)
    pub predicted_probability: f64,
    /// Actual result (0 or 1)
    pub actual_result: f64,
    /// Prediction error (absolute difference)
    pub prediction_error: f64,
    /// Squared error for Brier score calculation
    pub squared_error: f64,
    /// Confidence adjustment applied
    pub confidence_adjustment: f64,
    /// Timestamp when prediction was made
    pub predicted_at: i64,
    /// Timestamp when validation occurred
    pub validated_at: Option<i64>,
    /// Feedback source
    pub feedback_source: String,
    /// Claim content snippet
    pub claim_content: Option<String>,
    /// Claim type
    pub claim_type: String,
}

/// Statistics query result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionStats {
    pub total_predictions: u64,
    pub avg_error: f64,
    pub avg_squared_error: f64, // Brier score component
    pub accuracy: f64,
    pub error_std: f64,
}

/// Time-windowed statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowedStats {
    pub window_start: i64,
    pub window_end: i64,
    pub prediction_count: u64,
    pub avg_error: f64,
    pub accuracy: f64,
}

/// PredictionHistoryStore trait for time-series prediction error storage
#[async_trait]
pub trait PredictionHistoryStore: Send + Sync {
    /// Store a prediction error record
    async fn record_prediction(&self, record: &PredictionErrorRecord) -> Result<()>;

    /// Batch record predictions (high throughput)
    async fn batch_record(&self, records: &[PredictionErrorRecord]) -> Result<()>;

    /// Get statistics for a tenant
    async fn get_statistics(&self, tenant_id: &str) -> Result<PredictionStats>;

    /// Get time-windowed statistics
    async fn get_windowed_stats(
        &self,
        tenant_id: &str,
        window_seconds: i64,
        limit: usize,
    ) -> Result<Vec<WindowedStats>>;

    /// Get recent prediction errors
    async fn get_recent_errors(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<PredictionErrorRecord>>;

    /// Get errors above threshold
    async fn get_high_error_predictions(
        &self,
        tenant_id: &str,
        error_threshold: f64,
    ) -> Result<Vec<PredictionErrorRecord>>;

    /// Get prediction trend (last N points)
    async fn get_error_trend(&self, tenant_id: &str, points: usize) -> Result<Vec<(i64, f64)>>;
}

/// In-memory prediction history store (for development/testing)
pub struct InMemoryPredictionStore {
    records: Arc<RwLock<Vec<PredictionErrorRecord>>>,
}

impl InMemoryPredictionStore {
    pub fn new() -> Self {
        Self {
            records: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for InMemoryPredictionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PredictionHistoryStore for InMemoryPredictionStore {
    async fn record_prediction(&self, record: &PredictionErrorRecord) -> Result<()> {
        let mut records = self.records.write().await;
        records.push(record.clone());
        Ok(())
    }

    async fn batch_record(&self, records: &[PredictionErrorRecord]) -> Result<()> {
        let mut store = self.records.write().await;
        store.extend(records.iter().cloned());
        Ok(())
    }

    async fn get_statistics(&self, tenant_id: &str) -> Result<PredictionStats> {
        let records = self.records.read().await;
        let tenant_records: Vec<_> = records.iter().filter(|r| r.tenant_id == tenant_id).collect();
        
        if tenant_records.is_empty() {
            return Ok(PredictionStats {
                total_predictions: 0,
                avg_error: 0.0,
                avg_squared_error: 0.0,
                accuracy: 0.0,
                error_std: 0.0,
            });
        }

        let total = tenant_records.len() as f64;
        let avg_error: f64 = tenant_records.iter().map(|r| r.prediction_error).sum::<f64>() / total;
        let avg_squared_error: f64 = tenant_records.iter().map(|r| r.squared_error).sum::<f64>() / total;
        let accuracy: f64 = tenant_records.iter().filter(|r| {
            let predicted = if r.predicted_probability > 0.5 { 1.0 } else { 0.0 };
            predicted == r.actual_result
        }).count() as f64 / total;
        
        let variance: f64 = tenant_records.iter()
            .map(|r| (r.prediction_error - avg_error).powi(2))
            .sum::<f64>() / total;
        let error_std = variance.sqrt();

        Ok(PredictionStats {
            total_predictions: tenant_records.len() as u64,
            avg_error,
            avg_squared_error,
            accuracy,
            error_std,
        })
    }

    async fn get_windowed_stats(
        &self,
        tenant_id: &str,
        window_seconds: i64,
        limit: usize,
    ) -> Result<Vec<WindowedStats>> {
        let records = self.records.read().await;
        let tenant_records: Vec<_> = records.iter().filter(|r| r.tenant_id == tenant_id).collect();
        
        if tenant_records.is_empty() {
            return Ok(Vec::new());
        }

        let max_time = tenant_records.iter().map(|r| r.predicted_at).max().unwrap_or(0);
        let mut windows = Vec::new();
        
        for i in 0..limit {
            let window_end = max_time - (i as i64 * window_seconds);
            let window_start = window_end - window_seconds;
            
            let window_records: Vec<_> = tenant_records.iter()
                .filter(|r| r.predicted_at >= window_start && r.predicted_at < window_end)
                .collect();
            
            if window_records.is_empty() {
                continue;
            }
            
            let count = window_records.len() as f64;
            let avg_error: f64 = window_records.iter().map(|r| r.prediction_error).sum::<f64>() / count;
            let accuracy: f64 = window_records.iter().filter(|r| {
                let predicted = if r.predicted_probability > 0.5 { 1.0 } else { 0.0 };
                predicted == r.actual_result
            }).count() as f64 / count;

            windows.push(WindowedStats {
                window_start,
                window_end,
                prediction_count: window_records.len() as u64,
                avg_error,
                accuracy,
            });
        }

        Ok(windows)
    }

    async fn get_recent_errors(&self, tenant_id: &str, limit: usize) -> Result<Vec<PredictionErrorRecord>> {
        let records = self.records.read().await;
        let mut tenant_records: Vec<_> = records.iter()
            .filter(|r| r.tenant_id == tenant_id)
            .cloned()
            .collect();
        
        tenant_records.sort_by(|a, b| b.predicted_at.cmp(&a.predicted_at));
        tenant_records.truncate(limit);
        
        Ok(tenant_records)
    }

    async fn get_high_error_predictions(
        &self,
        tenant_id: &str,
        error_threshold: f64,
    ) -> Result<Vec<PredictionErrorRecord>> {
        let records = self.records.read().await;
        let tenant_records: Vec<_> = records.iter()
            .filter(|r| r.tenant_id == tenant_id && r.prediction_error > error_threshold)
            .cloned()
            .collect();
        
        Ok(tenant_records)
    }

    async fn get_error_trend(&self, tenant_id: &str, points: usize) -> Result<Vec<(i64, f64)>> {
        let records = self.records.read().await;
        let mut tenant_records: Vec<_> = records.iter()
            .filter(|r| r.tenant_id == tenant_id)
            .cloned()
            .collect();

        tenant_records.sort_by(|a, b| a.predicted_at.cmp(&b.predicted_at));

        let result: Vec<(i64, f64)> = tenant_records.iter()
            .take(points)
            .map(|r| (r.predicted_at, r.prediction_error))
            .collect();

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(tenant: &str, predicted: f64, actual: f64, time: i64) -> PredictionErrorRecord {
        let error = (predicted - actual).abs();
        PredictionErrorRecord {
            record_id: uuid::Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            claim_id: "claim-1".to_string(),
            validation_id: "val-1".to_string(),
            predicted_probability: predicted,
            actual_result: actual,
            prediction_error: error,
            squared_error: error * error,
            confidence_adjustment: 0.0,
            predicted_at: time,
            validated_at: Some(time + 100),
            feedback_source: "test".to_string(),
            claim_content: None,
            claim_type: "prediction".to_string(),
        }
    }

    #[tokio::test]
    async fn prediction_store_new_empty() {
        let store = InMemoryPredictionStore::new();
        let stats = store.get_statistics("tenant-1").await.unwrap();
        assert_eq!(stats.total_predictions, 0);
    }

    #[tokio::test]
    async fn prediction_store_record_and_get_stats() {
        let store = InMemoryPredictionStore::new();
        let record = make_record("tenant-1", 0.8, 1.0, 1000);
        store.record_prediction(&record).await.unwrap();

        let stats = store.get_statistics("tenant-1").await.unwrap();
        assert_eq!(stats.total_predictions, 1);
        assert!((stats.avg_error - 0.2).abs() < 1e-9);
    }

    #[tokio::test]
    async fn prediction_store_batch_record() {
        let store = InMemoryPredictionStore::new();
        let records = vec![
            make_record("tenant-1", 0.7, 1.0, 1000),
            make_record("tenant-1", 0.9, 1.0, 2000),
            make_record("tenant-1", 0.5, 0.0, 3000),
        ];
        store.batch_record(&records).await.unwrap();

        let stats = store.get_statistics("tenant-1").await.unwrap();
        assert_eq!(stats.total_predictions, 3);
    }

    #[tokio::test]
    async fn prediction_store_stats_accuracy() {
        let store = InMemoryPredictionStore::new();
        // predicted > 0.5 => predicted_class=1, actual=1 => correct
        let r1 = make_record("tenant-1", 0.8, 1.0, 1000);
        // predicted > 0.5 => predicted_class=1, actual=0 => wrong
        let r2 = make_record("tenant-1", 0.7, 0.0, 2000);
        // predicted < 0.5 => predicted_class=0, actual=0 => correct
        let r3 = make_record("tenant-1", 0.3, 0.0, 3000);
        // predicted < 0.5 => predicted_class=0, actual=1 => wrong
        let r4 = make_record("tenant-1", 0.2, 1.0, 4000);

        store.batch_record(&[r1, r2, r3, r4]).await.unwrap();

        let stats = store.get_statistics("tenant-1").await.unwrap();
        // 2 correct out of 4
        assert!((stats.accuracy - 0.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn prediction_store_stats_avg_error() {
        let store = InMemoryPredictionStore::new();
        // errors: 0.2, 0.1, 0.5 => avg = 0.8/3
        let records = vec![
            make_record("tenant-1", 0.8, 1.0, 1000), // error=0.2
            make_record("tenant-1", 0.9, 1.0, 2000), // error=0.1
            make_record("tenant-1", 0.5, 0.0, 3000), // error=0.5
        ];
        store.batch_record(&records).await.unwrap();

        let stats = store.get_statistics("tenant-1").await.unwrap();
        let expected_avg = (0.2 + 0.1 + 0.5) / 3.0;
        assert!((stats.avg_error - expected_avg).abs() < 1e-9);
    }

    #[tokio::test]
    async fn prediction_store_get_recent_errors() {
        let store = InMemoryPredictionStore::new();
        let records = vec![
            make_record("tenant-1", 0.8, 1.0, 1000),
            make_record("tenant-1", 0.9, 1.0, 3000),
            make_record("tenant-1", 0.5, 0.0, 2000),
        ];
        store.batch_record(&records).await.unwrap();

        let recent = store.get_recent_errors("tenant-1", 10).await.unwrap();
        // Should be sorted by predicted_at descending (most recent first)
        assert_eq!(recent[0].predicted_at, 3000);
        assert_eq!(recent[1].predicted_at, 2000);
        assert_eq!(recent[2].predicted_at, 1000);
    }

    #[tokio::test]
    async fn prediction_store_get_recent_errors_limit() {
        let store = InMemoryPredictionStore::new();
        let records = vec![
            make_record("tenant-1", 0.8, 1.0, 1000),
            make_record("tenant-1", 0.9, 1.0, 3000),
            make_record("tenant-1", 0.5, 0.0, 2000),
        ];
        store.batch_record(&records).await.unwrap();

        let recent = store.get_recent_errors("tenant-1", 2).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].predicted_at, 3000);
        assert_eq!(recent[1].predicted_at, 2000);
    }

    #[tokio::test]
    async fn prediction_store_get_high_error_predictions() {
        let store = InMemoryPredictionStore::new();
        let records = vec![
            make_record("tenant-1", 0.8, 1.0, 1000), // error=0.2
            make_record("tenant-1", 0.3, 1.0, 2000), // error=0.7
            make_record("tenant-1", 0.9, 1.0, 3000), // error=0.1
        ];
        store.batch_record(&records).await.unwrap();

        let high = store
            .get_high_error_predictions("tenant-1", 0.5)
            .await
            .unwrap();
        assert_eq!(high.len(), 1);
        assert!((high[0].prediction_error - 0.7).abs() < 1e-9);
    }

    #[tokio::test]
    async fn prediction_store_get_high_error_none_above_threshold() {
        let store = InMemoryPredictionStore::new();
        let records = vec![
            make_record("tenant-1", 0.8, 1.0, 1000), // error=0.2
            make_record("tenant-1", 0.9, 1.0, 2000), // error=0.1
        ];
        store.batch_record(&records).await.unwrap();

        let high = store
            .get_high_error_predictions("tenant-1", 0.5)
            .await
            .unwrap();
        assert!(high.is_empty());
    }

    #[tokio::test]
    async fn prediction_store_get_error_trend() {
        let store = InMemoryPredictionStore::new();
        let records = vec![
            make_record("tenant-1", 0.8, 1.0, 3000),
            make_record("tenant-1", 0.5, 0.0, 1000),
            make_record("tenant-1", 0.9, 1.0, 2000),
        ];
        store.batch_record(&records).await.unwrap();

        let trend = store.get_error_trend("tenant-1", 10).await.unwrap();
        // Should be sorted by time ascending
        assert_eq!(trend[0].0, 1000);
        assert_eq!(trend[1].0, 2000);
        assert_eq!(trend[2].0, 3000);
    }

    #[tokio::test]
    async fn prediction_store_get_windowed_stats() {
        let store = InMemoryPredictionStore::new();
        // Window size = 1000, records at times 500, 1500, 2500
        // max_time = 2500
        // window 0: [1500, 2500) => record at 1500 and 2500 is at boundary (2500 < 2500 is false)
        // Actually: window_end = 2500, window_start = 1500 => records where 1500 <= t < 2500
        let records = vec![
            make_record("tenant-1", 0.8, 1.0, 500),
            make_record("tenant-1", 0.9, 1.0, 1500),
            make_record("tenant-1", 0.6, 0.0, 2500),
        ];
        store.batch_record(&records).await.unwrap();

        let windows = store
            .get_windowed_stats("tenant-1", 1000, 3)
            .await
            .unwrap();
        // At least one window should have data
        assert!(!windows.is_empty());
        for w in &windows {
            assert!(w.prediction_count > 0);
        }
    }

    #[tokio::test]
    async fn prediction_store_tenant_isolation() {
        let store = InMemoryPredictionStore::new();
        store
            .record_prediction(&make_record("tenant-a", 0.8, 1.0, 1000))
            .await
            .unwrap();
        store
            .record_prediction(&make_record("tenant-b", 0.3, 0.0, 2000))
            .await
            .unwrap();
        store
            .record_prediction(&make_record("tenant-b", 0.7, 1.0, 3000))
            .await
            .unwrap();

        let stats_a = store.get_statistics("tenant-a").await.unwrap();
        let stats_b = store.get_statistics("tenant-b").await.unwrap();
        assert_eq!(stats_a.total_predictions, 1);
        assert_eq!(stats_b.total_predictions, 2);

        let recent_a = store.get_recent_errors("tenant-a", 10).await.unwrap();
        let recent_b = store.get_recent_errors("tenant-b", 10).await.unwrap();
        assert_eq!(recent_a.len(), 1);
        assert_eq!(recent_b.len(), 2);
    }

    #[tokio::test]
    async fn prediction_store_default() {
        let store = InMemoryPredictionStore::default();
        let stats = store.get_statistics("any").await.unwrap();
        assert_eq!(stats.total_predictions, 0);
    }
}
