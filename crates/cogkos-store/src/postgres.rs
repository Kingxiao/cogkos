//! PostgreSQL store implementation

use crate::{GapStore, KnowledgeGapRecord};
use async_trait::async_trait;
use cogkos_core::models::*;
use cogkos_core::{CogKosError, Result};
use sqlx::{PgPool, Row, postgres::PgRow};

/// Validate tenant_id format: must match `[a-z0-9_-]+` (no SQL injection risk).
fn validate_tenant_id(tenant_id: &str) -> Result<()> {
    if tenant_id.is_empty()
        || !tenant_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(CogKosError::InvalidInput(
            "Invalid tenant_id format: must match [a-z0-9_-]+".to_string(),
        ));
    }
    Ok(())
}

/// PostgreSQL store
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Set RLS tenant context on a connection.
    /// Must be called within a transaction for `SET LOCAL` to scope correctly.
    async fn set_tenant_context(conn: &mut sqlx::PgConnection, tenant_id: &str) -> Result<()> {
        validate_tenant_id(tenant_id)?;
        // SET LOCAL does not support $1 parameters, so we validate strictly above.
        sqlx::query(&format!("SET LOCAL app.current_tenant = '{}'", tenant_id))
            .execute(&mut *conn)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Create PostgresStore from database URL
    pub async fn from_url(url: &str) -> Result<Self> {
        let pool = PgPool::connect(url)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl super::ClaimStore for PostgresStore {
    #[tracing::instrument(skip(self, claim), fields(claim_id = %claim.id, tenant = %claim.tenant_id))]
    async fn insert_claim(&self, claim: &EpistemicClaim) -> Result<Id> {
        let db_start = std::time::Instant::now();
        let claimant_json =
            serde_json::to_value(&claim.claimant).map_err(CogKosError::Serialization)?;
        let provenance_json =
            serde_json::to_value(&claim.provenance).map_err(CogKosError::Serialization)?;
        let access_json =
            serde_json::to_value(&claim.access_envelope).map_err(CogKosError::Serialization)?;
        let metadata_json =
            serde_json::to_value(&claim.metadata).map_err(CogKosError::Serialization)?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), &claim.tenant_id).await?;

        sqlx::query(r#"
            INSERT INTO epistemic_claims (
                id, tenant_id, content, node_type, epistemic_status, confidence,
                consolidation_stage, claimant, provenance, access_envelope,
                activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                durability, created_at, updated_at, metadata
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)
        "#)
        .bind(claim.id)
        .bind(&claim.tenant_id)
        .bind(&claim.content)
        .bind(format!("{:?}", claim.node_type).to_lowercase())
        .bind(format!("{:?}", claim.epistemic_status).to_lowercase())
        .bind(claim.confidence)
        .bind(format!("{:?}", claim.consolidation_stage))
        .bind(claimant_json)
        .bind(provenance_json)
        .bind(access_json)
        .bind(claim.activation_weight)
        .bind(claim.access_count as i64)
        .bind(claim.last_accessed)
        .bind(claim.t_valid_start)
        .bind(claim.t_valid_end)
        .bind(claim.t_known)
        .bind(claim.vector_id)
        .bind(claim.last_prediction_error)
        .bind(&claim.derived_from)
        .bind(claim.needs_revalidation)
        .bind(claim.durability)
        .bind(claim.created_at)
        .bind(claim.updated_at)
        .bind(metadata_json)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        cogkos_core::monitoring::METRICS
            .record_duration("cogkos_db_insert_duration_seconds", db_start.elapsed());

        Ok(claim.id)
    }

    #[tracing::instrument(skip(self), fields(%id, %tenant_id))]
    async fn get_claim(&self, id: Id, tenant_id: &str) -> Result<EpistemicClaim> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                consolidation_stage, claimant, provenance, access_envelope,
                activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                durability, created_at, updated_at, metadata
            FROM epistemic_claims WHERE id = $1 AND tenant_id = $2
        "#,
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        match row {
            Some(row) => row_to_claim(&row),
            None => Err(CogKosError::NotFound(format!("Claim {} not found", id))),
        }
    }

    #[tracing::instrument(skip(self, claim), fields(claim_id = %claim.id, tenant = %claim.tenant_id))]
    async fn update_claim(&self, claim: &EpistemicClaim) -> Result<()> {
        let claimant_json =
            serde_json::to_value(&claim.claimant).map_err(CogKosError::Serialization)?;
        let provenance_json =
            serde_json::to_value(&claim.provenance).map_err(CogKosError::Serialization)?;
        let access_json =
            serde_json::to_value(&claim.access_envelope).map_err(CogKosError::Serialization)?;
        let metadata_json =
            serde_json::to_value(&claim.metadata).map_err(CogKosError::Serialization)?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), &claim.tenant_id).await?;

        sqlx::query(r#"
            UPDATE epistemic_claims SET
                content = $1, node_type = $2, epistemic_status = $3, confidence = $4,
                consolidation_stage = $5, claimant = $6, provenance = $7, access_envelope = $8,
                activation_weight = $9, access_count = $10, last_accessed = $11, t_valid_start = $12,
                t_valid_end = $13, t_known = $14, vector_id = $15, last_prediction_error = $16,
                derived_from = $17, needs_revalidation = $18, durability = $19, updated_at = $20,
                metadata = $21
            WHERE id = $22 AND tenant_id = $23
        "#)
        .bind(&claim.content)
        .bind(format!("{:?}", claim.node_type).to_lowercase())
        .bind(format!("{:?}", claim.epistemic_status).to_lowercase())
        .bind(claim.confidence)
        .bind(format!("{:?}", claim.consolidation_stage))
        .bind(claimant_json)
        .bind(provenance_json)
        .bind(access_json)
        .bind(claim.activation_weight)
        .bind(claim.access_count as i64)
        .bind(claim.last_accessed)
        .bind(claim.t_valid_start)
        .bind(claim.t_valid_end)
        .bind(claim.t_known)
        .bind(claim.vector_id)
        .bind(claim.last_prediction_error)
        .bind(&claim.derived_from)
        .bind(claim.needs_revalidation)
        .bind(claim.durability)
        .bind(claim.updated_at)
        .bind(metadata_json)
        .bind(claim.id)
        .bind(&claim.tenant_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    #[tracing::instrument(skip(self), fields(%id, %tenant_id))]
    async fn delete_claim(&self, id: Id, tenant_id: &str) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        sqlx::query("DELETE FROM epistemic_claims WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(tenant_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    #[tracing::instrument(skip(self, filters))]
    async fn query_claims(
        &self,
        tenant_id: &str,
        filters: &[QueryFilter],
    ) -> Result<Vec<EpistemicClaim>> {
        let mut query = String::from(
            r#"
            SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                consolidation_stage, claimant, provenance, access_envelope,
                activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                durability, created_at, updated_at, metadata
            FROM epistemic_claims WHERE tenant_id = $1
        "#,
        );

        // Build filter conditions
        for (i, filter) in filters.iter().enumerate() {
            match filter {
                QueryFilter::Stage { stage: _ } => {
                    query.push_str(&format!(" AND consolidation_stage = ${}", i + 2));
                }
                QueryFilter::Confidence { min: _, max: _ } => {
                    query.push_str(&format!(
                        " AND confidence >= ${} AND confidence <= ${}",
                        i + 2,
                        i + 3
                    ));
                }
                _ => {}
            }
        }

        query.push_str(" ORDER BY confidence DESC LIMIT 100");

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(&query)
            .bind(tenant_id)
            .fetch_all(tx.as_mut())
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_claim).collect()
    }

    async fn update_activation(&self, id: Id, tenant_id: &str, delta: f64) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        sqlx::query(
            r#"
            UPDATE epistemic_claims SET
                activation_weight = LEAST(1.0, activation_weight + $1),
                access_count = access_count + 1,
                last_accessed = NOW(),
                updated_at = NOW()
            WHERE id = $2
        "#,
        )
        .bind(delta)
        .bind(id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_conflicts_for_claim(
        &self,
        claim_id: Id,
        tenant_id: &str,
    ) -> Result<Vec<ConflictRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(r#"
            SELECT id, tenant_id, claim_a_id, claim_b_id, conflict_type, severity,
                description, detected_at, resolved_at, resolution, resolution_status, elevated_insight_id
            FROM conflict_records
            WHERE claim_a_id = $1 OR claim_b_id = $1
        "#)
        .bind(claim_id)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_conflict).collect()
    }

    async fn list_claims_by_stage(
        &self,
        tenant_id: &str,
        stage: cogkos_core::ConsolidationStage,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let stage_str = stage.to_string();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                consolidation_stage, claimant, provenance, access_envelope,
                activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                durability, created_at, updated_at, metadata
            FROM epistemic_claims
            WHERE tenant_id = $1 AND consolidation_stage = $2
            ORDER BY confidence DESC LIMIT $3
        "#,
        )
        .bind(tenant_id)
        .bind(stage_str)
        .bind(limit as i64)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_claim).collect()
    }

    #[tracing::instrument(skip(self))]
    async fn search_claims(
        &self,
        tenant_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let db_start = std::time::Instant::now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                consolidation_stage, claimant, provenance, access_envelope,
                activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                durability, created_at, updated_at, metadata
            FROM epistemic_claims
            WHERE tenant_id = $1 AND content LIKE $2
            ORDER BY confidence DESC LIMIT $3
        "#,
        )
        .bind(tenant_id)
        .bind(format!("%{}%", query))
        .bind(limit as i64)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        cogkos_core::monitoring::METRICS
            .record_duration("cogkos_db_search_duration_seconds", db_start.elapsed());

        rows.iter().map(row_to_claim).collect()
    }

    async fn update_confidence(&self, id: Id, tenant_id: &str, confidence: f64) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        sqlx::query(
            "UPDATE epistemic_claims SET confidence = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(confidence)
        .bind(id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_claims_needing_revalidation(
        &self,
        tenant_id: &str,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                consolidation_stage, claimant, provenance, access_envelope,
                activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                durability, created_at, updated_at, metadata
            FROM epistemic_claims
            WHERE tenant_id = $1 AND needs_revalidation = true AND confidence < $2
            ORDER BY confidence ASC LIMIT $3
        "#,
        )
        .bind(tenant_id)
        .bind(threshold)
        .bind(limit as i64)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_claim).collect()
    }

    async fn list_claims_needing_confidence_boost(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<EpistemicClaim>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, content, node_type, epistemic_status, confidence,
                consolidation_stage, claimant, provenance, access_envelope,
                activation_weight, access_count, last_accessed, t_valid_start, t_valid_end,
                t_known, vector_id, last_prediction_error, derived_from, needs_revalidation,
                durability, created_at, updated_at, metadata
            FROM epistemic_claims
            WHERE tenant_id = $1 AND metadata->>'needs_confidence_boost' = 'true'
            ORDER BY confidence ASC LIMIT $2
        "#,
        )
        .bind(tenant_id)
        .bind(limit as i64)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_claim).collect()
    }

    async fn list_tenants(&self) -> Result<Vec<String>> {
        let rows =
            sqlx::query_scalar::<_, String>("SELECT DISTINCT tenant_id FROM epistemic_claims")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(rows)
    }

    async fn insert_conflict(&self, conflict: &ConflictRecord) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), &conflict.tenant_id).await?;

        sqlx::query(r#"
            INSERT INTO conflict_records (
                id, tenant_id, claim_a_id, claim_b_id, conflict_type, severity,
                description, detected_at, resolved_at, resolution, resolution_status, elevated_insight_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#)
        .bind(conflict.id)
        .bind(&conflict.tenant_id)
        .bind(conflict.claim_a_id)
        .bind(conflict.claim_b_id)
        .bind(format!("{:?}", conflict.conflict_type))
        .bind(conflict.severity)
        .bind(&conflict.description)
        .bind(conflict.detected_at)
        .bind(conflict.resolved_at)
        .bind(&conflict.resolution)
        .bind(format!("{:?}", conflict.resolution_status))
        .bind(conflict.elevated_insight_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn resolve_conflict(
        &self,
        conflict_id: uuid::Uuid,
        tenant_id: &str,
        status: cogkos_core::models::ResolutionStatus,
        note: Option<String>,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        sqlx::query(
            r#"
            UPDATE conflict_records
            SET resolution_status = $1,
                resolution_note = $2,
                resolved_at = NOW()
            WHERE id = $3
        "#,
        )
        .bind(format!("{:?}", status))
        .bind(&note)
        .bind(conflict_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }
}

fn row_to_claim(row: &PgRow) -> Result<EpistemicClaim> {
    use sqlx::Row;

    let claimant: serde_json::Value = row
        .try_get("claimant")
        .map_err(|e| CogKosError::Database(e.to_string()))?;
    let provenance: serde_json::Value = row
        .try_get("provenance")
        .map_err(|e| CogKosError::Database(e.to_string()))?;
    let access: serde_json::Value = row
        .try_get("access_envelope")
        .map_err(|e| CogKosError::Database(e.to_string()))?;
    let metadata: serde_json::Value = row
        .try_get("metadata")
        .map_err(|e| CogKosError::Database(e.to_string()))?;

    Ok(EpistemicClaim {
        id: row
            .try_get("id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        tenant_id: row
            .try_get("tenant_id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        content: row
            .try_get("content")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        node_type: parse_node_type(&row.try_get::<String, _>("node_type").unwrap_or_default()),
        knowledge_type: cogkos_core::models::KnowledgeType::Experiential,
        structured_content: None,
        epistemic_status: parse_epistemic_status(
            &row.try_get::<String, _>("epistemic_status")
                .unwrap_or_default(),
        ),
        confidence: row
            .try_get("confidence")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        consolidation_stage: parse_consolidation_stage(
            &row.try_get::<String, _>("consolidation_stage")
                .unwrap_or_default(),
        ),
        claimant: serde_json::from_value(claimant).map_err(CogKosError::Serialization)?,
        provenance: serde_json::from_value(provenance).map_err(CogKosError::Serialization)?,
        access_envelope: serde_json::from_value(access).map_err(CogKosError::Serialization)?,
        activation_weight: row
            .try_get("activation_weight")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        access_count: row.try_get::<i64, _>("access_count").unwrap_or(0) as u64,
        last_accessed: row.try_get("last_accessed").ok(),
        t_valid_start: row
            .try_get("t_valid_start")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        t_valid_end: row.try_get("t_valid_end").ok(),
        t_known: row
            .try_get("t_known")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        vector_id: row.try_get("vector_id").ok(),
        last_prediction_error: row.try_get("last_prediction_error").ok(),
        derived_from: row.try_get("derived_from").unwrap_or_default(),
        superseded_by: row.try_get("superseded_by").ok(),
        entity_refs: row
            .try_get::<sqlx::types::Json<Vec<cogkos_core::models::EntityRef>>, _>("entity_refs")
            .map(|v| v.0)
            .unwrap_or_default(),
        needs_revalidation: row.try_get("needs_revalidation").unwrap_or(false),
        version: 1,
        durability: row.try_get("durability").unwrap_or(1.0),
        created_at: row
            .try_get("created_at")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        metadata: serde_json::from_value(metadata).unwrap_or_default(),
    })
}

fn row_to_conflict(row: &PgRow) -> Result<ConflictRecord> {
    use sqlx::Row;

    let resolution: Option<serde_json::Value> = row.try_get("resolution").ok();

    Ok(ConflictRecord {
        id: row
            .try_get("id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        tenant_id: row
            .try_get("tenant_id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        claim_a_id: row
            .try_get("claim_a_id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        claim_b_id: row
            .try_get("claim_b_id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        conflict_type: parse_conflict_type(
            &row.try_get::<String, _>("conflict_type")
                .unwrap_or_default(),
        ),
        severity: row.try_get("severity").unwrap_or(0.5),
        description: row.try_get("description").unwrap_or_default(),
        detected_at: row
            .try_get("detected_at")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        resolved_at: row.try_get("resolved_at").ok(),
        resolution: resolution.and_then(|r| serde_json::from_value(r).ok()),
        resolution_status: parse_resolution_status(
            &row.try_get::<String, _>("resolution_status")
                .unwrap_or_default(),
        ),
        resolution_note: row.try_get("resolution_note").unwrap_or_default(),
        elevated_insight_id: row.try_get("elevated_insight_id").ok(),
    })
}

fn parse_node_type(s: &str) -> NodeType {
    match s.to_lowercase().as_str() {
        "entity" => NodeType::Entity,
        "relation" => NodeType::Relation,
        "event" => NodeType::Event,
        "attribute" => NodeType::Attribute,
        "prediction" => NodeType::Prediction,
        "insight" => NodeType::Insight,
        "file" => NodeType::File,
        _ => NodeType::Entity,
    }
}

fn parse_epistemic_status(s: &str) -> EpistemicStatus {
    match s.to_lowercase().as_str() {
        "asserted" => EpistemicStatus::Asserted,
        "corroborated" => EpistemicStatus::Corroborated,
        "contested" => EpistemicStatus::Contested,
        "retracted" => EpistemicStatus::Retracted,
        "superseded" => EpistemicStatus::Superseded,
        _ => EpistemicStatus::Asserted,
    }
}

fn parse_consolidation_stage(s: &str) -> ConsolidationStage {
    match s {
        "FastTrack" => ConsolidationStage::FastTrack,
        "Consolidated" => ConsolidationStage::Consolidated,
        "Insight" => ConsolidationStage::Insight,
        "Archived" => ConsolidationStage::Archived,
        _ => ConsolidationStage::FastTrack,
    }
}

fn parse_conflict_type(s: &str) -> ConflictType {
    match s.to_lowercase().as_str() {
        "directcontradiction" => ConflictType::DirectContradiction,
        "temporalinconsistency" => ConflictType::TemporalInconsistency,
        "confidencemismatch" => ConflictType::ConfidenceMismatch,
        "contextualdifference" => ConflictType::ContextualDifference,
        "sourcedisagreement" => ConflictType::SourceDisagreement,
        "temporalshift" => ConflictType::TemporalShift,
        _ => ConflictType::DirectContradiction,
    }
}

fn parse_resolution_status(s: &str) -> ResolutionStatus {
    match s.to_lowercase().as_str() {
        "open" => ResolutionStatus::Open,
        "elevated" => ResolutionStatus::Elevated,
        "dismissed" => ResolutionStatus::Dismissed,
        "accepted" => ResolutionStatus::Accepted,
        _ => ResolutionStatus::Open,
    }
}

#[async_trait]
impl GapStore for PostgresStore {
    async fn record_gap(&self, gap: &KnowledgeGapRecord) -> Result<uuid::Uuid> {
        let gap_id = gap.gap_id;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), &gap.tenant_id).await?;

        sqlx::query(
            r#"
            INSERT INTO knowledge_gaps (gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (tenant_id, domain, description)
            DO UPDATE SET status = 'open', reported_at = NOW()
            RETURNING gap_id
            "#,
        )
        .bind(gap.gap_id)
        .bind(&gap.tenant_id)
        .bind(&gap.domain)
        .bind(&gap.description)
        .bind(&gap.priority)
        .bind(&gap.status)
        .bind(gap.reported_at)
        .bind(gap.filled_at)
        .fetch_one(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(gap_id)
    }

    async fn find_similar_gap(
        &self,
        tenant_id: &str,
        domain: &str,
        description: &str,
    ) -> Result<Option<KnowledgeGapRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let result = sqlx::query(
            r#"
            SELECT gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at
            FROM knowledge_gaps
            WHERE tenant_id = $1 AND domain = $2 AND description = $3 AND status = 'open'
            "#,
        )
        .bind(tenant_id)
        .bind(domain)
        .bind(description)
        .fetch_optional(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(result.map(|row| KnowledgeGapRecord {
            gap_id: row.get("gap_id"),
            tenant_id: row.get("tenant_id"),
            domain: row.get("domain"),
            description: row.get("description"),
            priority: row.get("priority"),
            status: row.get("status"),
            reported_at: row.get("reported_at"),
            filled_at: row.get("filled_at"),
        }))
    }

    async fn get_gaps(&self, tenant_id: &str) -> Result<Vec<KnowledgeGapRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let results = sqlx::query(
            r#"
            SELECT gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at
            FROM knowledge_gaps
            WHERE tenant_id = $1
            ORDER BY reported_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|row| KnowledgeGapRecord {
                gap_id: row.get("gap_id"),
                tenant_id: row.get("tenant_id"),
                domain: row.get("domain"),
                description: row.get("description"),
                priority: row.get("priority"),
                status: row.get("status"),
                reported_at: row.get("reported_at"),
                filled_at: row.get("filled_at"),
            })
            .collect())
    }

    async fn get_gaps_by_domain(
        &self,
        tenant_id: &str,
        domain: &str,
    ) -> Result<Vec<KnowledgeGapRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let results = sqlx::query(
            r#"
            SELECT gap_id, tenant_id, domain, description, priority, status, reported_at, filled_at
            FROM knowledge_gaps
            WHERE tenant_id = $1 AND domain = $2
            ORDER BY reported_at DESC
            "#,
        )
        .bind(tenant_id)
        .bind(domain)
        .fetch_all(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|row| KnowledgeGapRecord {
                gap_id: row.get("gap_id"),
                tenant_id: row.get("tenant_id"),
                domain: row.get("domain"),
                description: row.get("description"),
                priority: row.get("priority"),
                status: row.get("status"),
                reported_at: row.get("reported_at"),
                filled_at: row.get("filled_at"),
            })
            .collect())
    }

    async fn mark_gap_filled(&self, gap_id: uuid::Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE knowledge_gaps
            SET status = 'filled', filled_at = NOW()
            WHERE gap_id = $1
            "#,
        )
        .bind(gap_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }
}

/// PostgreSQL gap record helper
struct PgGapRecord {
    gap_id: uuid::Uuid,
    tenant_id: String,
    domain: String,
    description: String,
    priority: String,
    status: String,
    reported_at: chrono::DateTime<chrono::Utc>,
    filled_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<PgGapRecord> for KnowledgeGapRecord {
    fn from(r: PgGapRecord) -> Self {
        Self {
            gap_id: r.gap_id,
            tenant_id: r.tenant_id,
            domain: r.domain,
            description: r.description,
            priority: r.priority,
            status: r.status,
            reported_at: r.reported_at,
            filled_at: r.filled_at,
        }
    }
}

// ============ Auth Store Implementation ============

#[async_trait]
impl super::AuthStore for PostgresStore {
    async fn validate_api_key(&self, api_key: &str) -> Result<(String, Vec<String>)> {
        let result = sqlx::query(
            "SELECT tenant_id, permissions FROM api_keys
             WHERE key_hash = crypt($1, key_hash) AND enabled = true
             AND (expires_at IS NULL OR expires_at > NOW())",
        )
        .bind(api_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        match result {
            Some(row) => {
                let tenant_id: String = row.get("tenant_id");
                let permissions: Vec<String> = row.get("permissions");

                Ok((tenant_id, permissions))
            }
            None => Err(CogKosError::AccessDenied("Invalid API key".to_string())),
        }
    }

    async fn create_api_key(&self, tenant_id: &str, permissions: Vec<String>) -> Result<String> {
        let api_key = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO api_keys (key_hash, tenant_id, name, permissions)
             VALUES (crypt($1, gen_salt('bf')), $2, 'auto-generated', $3)",
        )
        .bind(&api_key)
        .bind(tenant_id)
        .bind(&permissions)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(api_key)
    }

    async fn revoke_api_key(&self, key_hash: &str) -> Result<()> {
        sqlx::query("UPDATE api_keys SET enabled = false WHERE key_hash = $1")
            .bind(key_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Ok(())
    }
}

// ============ Cache Store Implementation ============

#[async_trait]
impl super::CacheStore for PostgresStore {
    async fn get_cached(
        &self,
        tenant_id: &str,
        query_hash: u64,
    ) -> Result<Option<cogkos_core::models::QueryCacheEntry>> {
        let row = sqlx::query(
            r#"
            SELECT tenant_id, query_hash, response, confidence, hit_count, success_count,
                   last_used, created_at, invalidated_by
            FROM query_cache
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let response: serde_json::Value = row
                    .try_get("response")
                    .map_err(|e| CogKosError::Database(e.to_string()))?;
                let response: cogkos_core::McpQueryResponse =
                    serde_json::from_value(response).map_err(CogKosError::Serialization)?;

                Ok(Some(cogkos_core::models::QueryCacheEntry {
                    query_hash: row.try_get::<i64, _>("query_hash").unwrap_or(0) as u64,
                    response,
                    confidence: row.try_get("confidence").unwrap_or(0.6),
                    hit_count: row.try_get::<i64, _>("hit_count").unwrap_or(0) as u64,
                    success_count: row.try_get::<i64, _>("success_count").unwrap_or(0) as u64,
                    last_used: row
                        .try_get("last_used")
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    created_at: row
                        .try_get("created_at")
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    invalidated_by: row.try_get("invalidated_by").ok(),
                }))
            }
            None => Ok(None),
        }
    }

    async fn set_cached(
        &self,
        tenant_id: &str,
        entry: &cogkos_core::models::QueryCacheEntry,
    ) -> Result<()> {
        let response_json =
            serde_json::to_value(&entry.response).map_err(CogKosError::Serialization)?;

        sqlx::query(
            r#"
            INSERT INTO query_cache (tenant_id, query_hash, response, confidence, hit_count, success_count, last_used, created_at, invalidated_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (tenant_id, query_hash)
            DO UPDATE SET response = $3, confidence = $4, last_used = NOW()
            "#,
        )
        .bind(tenant_id)
        .bind(entry.query_hash as i64)
        .bind(response_json)
        .bind(entry.confidence)
        .bind(entry.hit_count as i64)
        .bind(entry.success_count as i64)
        .bind(entry.last_used)
        .bind(entry.created_at)
        .bind(entry.invalidated_by)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn record_hit(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE query_cache
            SET hit_count = hit_count + 1, last_used = NOW()
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn record_success(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE query_cache
            SET success_count = success_count + 1
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn invalidate(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query("DELETE FROM query_cache WHERE tenant_id = $1 AND query_hash = $2")
            .bind(tenant_id)
            .bind(query_hash as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn refresh_ttl(&self, tenant_id: &str, query_hash: u64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE query_cache
            SET last_used = NOW()
            WHERE tenant_id = $1 AND query_hash = $2
            "#,
        )
        .bind(tenant_id)
        .bind(query_hash as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }
}

// ============ Feedback Store Implementation ============

#[async_trait]
impl super::FeedbackStore for PostgresStore {
    async fn insert_feedback(&self, feedback: &cogkos_core::models::AgentFeedback) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO agent_feedbacks (query_hash, agent_id, success, feedback_note, timestamp)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(feedback.query_hash as i64)
        .bind(&feedback.agent_id)
        .bind(feedback.success)
        .bind(&feedback.feedback_note)
        .bind(feedback.timestamp)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_feedback_for_query(
        &self,
        query_hash: u64,
    ) -> Result<Vec<cogkos_core::models::AgentFeedback>> {
        let rows = sqlx::query(
            r#"
            SELECT id, query_hash, agent_id, success, feedback_note, timestamp
            FROM agent_feedbacks
            WHERE query_hash = $1
            ORDER BY timestamp DESC
            "#,
        )
        .bind(query_hash as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter()
            .map(|row| {
                Ok(cogkos_core::models::AgentFeedback {
                    query_hash: row.try_get::<i64, _>("query_hash").unwrap_or(0) as u64,
                    agent_id: row.get("agent_id"),
                    success: row.get("success"),
                    feedback_note: row.try_get("feedback_note").ok(),
                    timestamp: row.get("timestamp"),
                })
            })
            .collect()
    }
}

// ============ Subscription Store Implementation ============

#[async_trait]
impl super::SubscriptionStore for PostgresStore {
    async fn create_subscription(
        &self,
        subscription: &cogkos_core::models::SubscriptionSource,
    ) -> Result<uuid::Uuid> {
        let config_json =
            serde_json::to_value(&subscription.config).map_err(CogKosError::Serialization)?;
        let claimant_json = serde_json::to_value(&subscription.claimant_template)
            .map_err(CogKosError::Serialization)?;

        sqlx::query(
            r#"
            INSERT INTO subscriptions (
                id, tenant_id, name, source_type, config, poll_interval_secs,
                claimant_template, base_confidence, enabled, last_polled, error_count,
                created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW(), NOW())
        "#,
        )
        .bind(subscription.id)
        .bind(&subscription.tenant_id)
        .bind(&subscription.name)
        .bind(format!("{:?}", subscription.source_type).to_lowercase())
        .bind(config_json)
        .bind(subscription.poll_interval_secs as i64)
        .bind(claimant_json)
        .bind(subscription.base_confidence)
        .bind(subscription.enabled)
        .bind(subscription.last_polled)
        .bind(subscription.error_count as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(subscription.id)
    }

    async fn get_subscription(
        &self,
        tenant_id: &str,
        id: uuid::Uuid,
    ) -> Result<cogkos_core::models::SubscriptionSource> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, name, source_type, config, poll_interval_secs,
                   claimant_template, base_confidence, enabled, last_polled, error_count,
                   created_at, updated_at
            FROM subscriptions
            WHERE id = $1 AND tenant_id = $2
            "#,
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        match row {
            Some(row) => row_to_subscription(&row),
            None => Err(CogKosError::NotFound(format!(
                "Subscription {} not found",
                id
            ))),
        }
    }

    async fn update_subscription(
        &self,
        subscription: &cogkos_core::models::SubscriptionSource,
    ) -> Result<()> {
        let config_json =
            serde_json::to_value(&subscription.config).map_err(CogKosError::Serialization)?;
        let claimant_json = serde_json::to_value(&subscription.claimant_template)
            .map_err(CogKosError::Serialization)?;

        sqlx::query(
            r#"
            UPDATE subscriptions SET
                name = $1,
                source_type = $2,
                config = $3,
                poll_interval_secs = $4,
                claimant_template = $5,
                base_confidence = $6,
                enabled = $7,
                last_polled = $8,
                error_count = $9,
                updated_at = NOW()
            WHERE id = $10 AND tenant_id = $11
        "#,
        )
        .bind(&subscription.name)
        .bind(format!("{:?}", subscription.source_type).to_lowercase())
        .bind(config_json)
        .bind(subscription.poll_interval_secs as i64)
        .bind(claimant_json)
        .bind(subscription.base_confidence)
        .bind(subscription.enabled)
        .bind(subscription.last_polled)
        .bind(subscription.error_count as i64)
        .bind(subscription.id)
        .bind(&subscription.tenant_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn delete_subscription(&self, tenant_id: &str, id: uuid::Uuid) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(tenant_id)
            .execute(&self.pool)
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_subscriptions(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<cogkos_core::models::SubscriptionSource>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, name, source_type, config, poll_interval_secs,
                   claimant_template, base_confidence, enabled, last_polled, error_count,
                   created_at, updated_at
            FROM subscriptions
            WHERE tenant_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_subscription).collect()
    }

    async fn list_enabled_subscriptions(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<cogkos_core::models::SubscriptionSource>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, name, source_type, config, poll_interval_secs,
                   claimant_template, base_confidence, enabled, last_polled, error_count,
                   created_at, updated_at
            FROM subscriptions
            WHERE tenant_id = $1 AND enabled = true
            ORDER BY created_at DESC
            "#,
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        rows.iter().map(row_to_subscription).collect()
    }

    async fn update_subscription_status(&self, id: uuid::Uuid, _status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET last_polled = NOW(), updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn increment_error_count(&self, id: uuid::Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET error_count = error_count + 1, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    async fn reset_error_count(&self, id: uuid::Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE subscriptions
            SET error_count = 0, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }
}

/// Convert a database row to a SubscriptionSource
fn row_to_subscription(row: &PgRow) -> Result<cogkos_core::models::SubscriptionSource> {
    use sqlx::Row;

    let config: serde_json::Value = row
        .try_get("config")
        .map_err(|e| CogKosError::Database(e.to_string()))?;
    let claimant: serde_json::Value = row
        .try_get("claimant_template")
        .map_err(|e| CogKosError::Database(e.to_string()))?;
    let source_type_str: String = row
        .try_get("source_type")
        .map_err(|e| CogKosError::Database(e.to_string()))?;

    Ok(cogkos_core::models::SubscriptionSource {
        id: row
            .try_get("id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        tenant_id: row
            .try_get("tenant_id")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        name: row
            .try_get("name")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        source_type: parse_subscription_type(&source_type_str),
        config: serde_json::from_value(config).map_err(CogKosError::Serialization)?,
        poll_interval_secs: row.try_get::<i64, _>("poll_interval_secs").unwrap_or(3600) as u64,
        claimant_template: serde_json::from_value(claimant).map_err(CogKosError::Serialization)?,
        base_confidence: row
            .try_get("base_confidence")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        enabled: row
            .try_get("enabled")
            .map_err(|e| CogKosError::Database(e.to_string()))?,
        last_polled: row.try_get("last_polled").ok(),
        error_count: row.try_get::<i64, _>("error_count").unwrap_or(0) as u32,
    })
}

/// Parse subscription type from string
fn parse_subscription_type(s: &str) -> cogkos_core::models::SubscriptionType {
    match s.to_lowercase().as_str() {
        "rss" => cogkos_core::models::SubscriptionType::Rss,
        "webhook" => cogkos_core::models::SubscriptionType::Webhook,
        _ => cogkos_core::models::SubscriptionType::Rss,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_tenant_id_valid() {
        assert!(validate_tenant_id("tenant-1").is_ok());
        assert!(validate_tenant_id("my_org").is_ok());
        assert!(validate_tenant_id("abc123").is_ok());
        assert!(validate_tenant_id("a-b_c-d").is_ok());
    }

    #[test]
    fn test_validate_tenant_id_rejects_empty() {
        assert!(validate_tenant_id("").is_err());
    }

    #[test]
    fn test_validate_tenant_id_rejects_uppercase() {
        assert!(validate_tenant_id("Tenant").is_err());
        assert!(validate_tenant_id("UPPER").is_err());
    }

    #[test]
    fn test_validate_tenant_id_rejects_sql_injection() {
        assert!(validate_tenant_id("'; DROP TABLE --").is_err());
        assert!(validate_tenant_id("tenant' OR '1'='1").is_err());
        assert!(validate_tenant_id("tenant; DELETE FROM").is_err());
    }

    #[test]
    fn test_validate_tenant_id_rejects_special_chars() {
        assert!(validate_tenant_id("tenant.org").is_err());
        assert!(validate_tenant_id("tenant@org").is_err());
        assert!(validate_tenant_id("tenant/org").is_err());
        assert!(validate_tenant_id("tenant org").is_err());
    }
}
