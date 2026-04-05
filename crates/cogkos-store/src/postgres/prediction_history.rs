//! PostgreSQL implementation of PredictionHistoryStore

use super::PostgresStore;
use crate::prediction_history::{
    PredictionErrorRecord, PredictionHistoryStore, PredictionStats, WindowedStats,
};
use async_trait::async_trait;
use cogkos_core::{CogKosError, Result};
use sqlx::Row;

#[async_trait]
impl PredictionHistoryStore for PostgresStore {
    async fn record_prediction(&self, record: &PredictionErrorRecord) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), &record.tenant_id).await?;

        sqlx::query(
            r#"INSERT INTO prediction_history
                (record_id, tenant_id, claim_id, validation_id,
                 predicted_probability, actual_result, prediction_error, squared_error,
                 confidence_adjustment, predicted_at, validated_at,
                 feedback_source, claim_content, claim_type)
            VALUES ($1, $2, $3::uuid, $4, $5, $6, $7, $8, $9,
                    to_timestamp($10::bigint),
                    CASE WHEN $11::bigint IS NOT NULL THEN to_timestamp($11::bigint) ELSE NULL END,
                    $12, $13, $14)"#,
        )
        .bind(&record.record_id)
        .bind(&record.tenant_id)
        .bind(&record.claim_id)
        .bind(&record.validation_id)
        .bind(record.predicted_probability)
        .bind(record.actual_result != 0.0)
        .bind(record.prediction_error)
        .bind(record.squared_error)
        .bind(record.confidence_adjustment)
        .bind(record.predicted_at)
        .bind(record.validated_at)
        .bind(&record.feedback_source)
        .bind(&record.claim_content)
        .bind(&record.claim_type)
        .execute(&mut *tx)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }

    async fn batch_record(&self, records: &[PredictionErrorRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        // Group by tenant for RLS context
        let tenant_id = &records[0].tenant_id;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        for record in records {
            sqlx::query(
                r#"INSERT INTO prediction_history
                    (record_id, tenant_id, claim_id, validation_id,
                     predicted_probability, actual_result, prediction_error, squared_error,
                     confidence_adjustment, predicted_at, validated_at,
                     feedback_source, claim_content, claim_type)
                VALUES ($1, $2, $3::uuid, $4, $5, $6, $7, $8, $9,
                        to_timestamp($10::bigint),
                        CASE WHEN $11::bigint IS NOT NULL THEN to_timestamp($11::bigint) ELSE NULL END,
                        $12, $13, $14)"#,
            )
            .bind(&record.record_id)
            .bind(&record.tenant_id)
            .bind(&record.claim_id)
            .bind(&record.validation_id)
            .bind(record.predicted_probability)
            .bind(record.actual_result != 0.0)
            .bind(record.prediction_error)
            .bind(record.squared_error)
            .bind(record.confidence_adjustment)
            .bind(record.predicted_at)
            .bind(record.validated_at)
            .bind(&record.feedback_source)
            .bind(&record.claim_content)
            .bind(&record.claim_type)
            .execute(&mut *tx)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get_statistics(&self, tenant_id: &str) -> Result<PredictionStats> {
        super::validate_tenant_id(tenant_id)?;
        let row = sqlx::query(
            r#"SELECT
                COUNT(*) as total,
                COALESCE(AVG(prediction_error), 0) as avg_error,
                COALESCE(AVG(squared_error), 0) as avg_sq_error,
                COALESCE(STDDEV_POP(prediction_error), 0) as error_std,
                COALESCE(
                    AVG(CASE WHEN (predicted_probability > 0.5 AND actual_result = true)
                             OR (predicted_probability <= 0.5 AND actual_result = false)
                        THEN 1.0 ELSE 0.0 END),
                    0
                ) as accuracy
            FROM prediction_history
            WHERE tenant_id = $1"#,
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(PredictionStats {
            total_predictions: row.get::<i64, _>("total").try_into().unwrap_or(0),
            avg_error: row.get::<f64, _>("avg_error"),
            avg_squared_error: row.get::<f64, _>("avg_sq_error"),
            accuracy: row.get::<f64, _>("accuracy"),
            error_std: row.get::<f64, _>("error_std"),
        })
    }

    async fn get_windowed_stats(
        &self,
        tenant_id: &str,
        window_seconds: i64,
        limit: usize,
    ) -> Result<Vec<WindowedStats>> {
        super::validate_tenant_id(tenant_id)?;
        let rows = sqlx::query(
            r#"WITH windows AS (
                SELECT
                    floor(extract(epoch from predicted_at) / $2) * $2 as window_start,
                    prediction_error,
                    predicted_probability,
                    actual_result
                FROM prediction_history
                WHERE tenant_id = $1
            )
            SELECT
                window_start::bigint,
                (window_start + $2)::bigint as window_end,
                COUNT(*) as cnt,
                AVG(prediction_error) as avg_error,
                AVG(CASE WHEN (predicted_probability > 0.5 AND actual_result = true)
                         OR (predicted_probability <= 0.5 AND actual_result = false)
                    THEN 1.0 ELSE 0.0 END) as accuracy
            FROM windows
            GROUP BY window_start
            ORDER BY window_start DESC
            LIMIT $3"#,
        )
        .bind(tenant_id)
        .bind(window_seconds as f64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|r| WindowedStats {
                window_start: r.get::<i64, _>("window_start"),
                window_end: r.get::<i64, _>("window_end"),
                prediction_count: r.get::<i64, _>("cnt") as u64,
                avg_error: r.get::<f64, _>("avg_error"),
                accuracy: r.get::<f64, _>("accuracy"),
            })
            .collect())
    }

    async fn get_recent_errors(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<PredictionErrorRecord>> {
        super::validate_tenant_id(tenant_id)?;
        let rows = sqlx::query(
            r#"SELECT record_id, tenant_id, claim_id::text, validation_id,
                      predicted_probability, actual_result, prediction_error, squared_error,
                      confidence_adjustment,
                      extract(epoch from predicted_at)::bigint as predicted_epoch,
                      extract(epoch from validated_at)::bigint as validated_epoch,
                      feedback_source, claim_content, claim_type
            FROM prediction_history
            WHERE tenant_id = $1
            ORDER BY predicted_at DESC
            LIMIT $2"#,
        )
        .bind(tenant_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(rows.iter().map(row_to_record).collect())
    }

    async fn get_high_error_predictions(
        &self,
        tenant_id: &str,
        error_threshold: f64,
    ) -> Result<Vec<PredictionErrorRecord>> {
        super::validate_tenant_id(tenant_id)?;
        let rows = sqlx::query(
            r#"SELECT record_id, tenant_id, claim_id::text, validation_id,
                      predicted_probability, actual_result, prediction_error, squared_error,
                      confidence_adjustment,
                      extract(epoch from predicted_at)::bigint as predicted_epoch,
                      extract(epoch from validated_at)::bigint as validated_epoch,
                      feedback_source, claim_content, claim_type
            FROM prediction_history
            WHERE tenant_id = $1 AND prediction_error > $2
            ORDER BY prediction_error DESC"#,
        )
        .bind(tenant_id)
        .bind(error_threshold)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(rows.iter().map(row_to_record).collect())
    }

    async fn get_error_trend(&self, tenant_id: &str, points: usize) -> Result<Vec<(i64, f64)>> {
        super::validate_tenant_id(tenant_id)?;
        let rows = sqlx::query(
            r#"SELECT extract(epoch from predicted_at)::bigint as ts, prediction_error
            FROM prediction_history
            WHERE tenant_id = $1
            ORDER BY predicted_at ASC
            LIMIT $2"#,
        )
        .bind(tenant_id)
        .bind(points as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|r| (r.get::<i64, _>("ts"), r.get::<f64, _>("prediction_error")))
            .collect())
    }
}

fn row_to_record(row: &sqlx::postgres::PgRow) -> PredictionErrorRecord {
    PredictionErrorRecord {
        record_id: row
            .get::<Option<String>, _>("record_id")
            .unwrap_or_default(),
        tenant_id: row.get::<String, _>("tenant_id"),
        claim_id: row.get::<Option<String>, _>("claim_id").unwrap_or_default(),
        validation_id: row
            .get::<Option<String>, _>("validation_id")
            .unwrap_or_default(),
        predicted_probability: row
            .get::<Option<f64>, _>("predicted_probability")
            .unwrap_or(0.0),
        actual_result: if row.get::<Option<bool>, _>("actual_result").unwrap_or(false) {
            1.0
        } else {
            0.0
        },
        prediction_error: row.get::<Option<f64>, _>("prediction_error").unwrap_or(0.0),
        squared_error: row.get::<Option<f64>, _>("squared_error").unwrap_or(0.0),
        confidence_adjustment: row
            .get::<Option<f64>, _>("confidence_adjustment")
            .unwrap_or(0.0),
        predicted_at: row.get::<i64, _>("predicted_epoch"),
        validated_at: row.get::<Option<i64>, _>("validated_epoch"),
        feedback_source: row
            .get::<Option<String>, _>("feedback_source")
            .unwrap_or_default(),
        claim_content: row.get::<Option<String>, _>("claim_content"),
        claim_type: row
            .get::<Option<String>, _>("claim_type")
            .unwrap_or_default(),
    }
}
