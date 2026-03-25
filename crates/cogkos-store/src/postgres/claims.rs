//! ClaimStore implementation for PostgresStore

use super::PostgresStore;
use async_trait::async_trait;
use cogkos_core::models::*;
use cogkos_core::{CogKosError, Result};
use sqlx::postgres::PgRow;

#[async_trait]
impl crate::ClaimStore for PostgresStore {
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
        .bind(claim.node_type.as_db_str())
        .bind(claim.epistemic_status.as_db_str())
        .bind(claim.confidence)
        .bind(claim.consolidation_stage.as_db_str())
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
        .bind(claim.node_type.as_db_str())
        .bind(claim.epistemic_status.as_db_str())
        .bind(claim.confidence)
        .bind(claim.consolidation_stage.as_db_str())
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
            WHERE id = $2 AND tenant_id = $3
        "#,
        )
        .bind(delta)
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
            WHERE tenant_id = $2 AND (claim_a_id = $1 OR claim_b_id = $1)
        "#)
        .bind(claim_id)
        .bind(tenant_id)
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
            "UPDATE epistemic_claims SET confidence = $1, updated_at = NOW() WHERE id = $2 AND tenant_id = $3",
        )
        .bind(confidence)
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
        .bind(conflict.conflict_type.as_db_str())
        .bind(conflict.severity)
        .bind(&conflict.description)
        .bind(conflict.detected_at)
        .bind(conflict.resolved_at)
        .bind(&conflict.resolution)
        .bind(conflict.resolution_status.as_db_str())
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
            WHERE id = $3 AND tenant_id = $4
        "#,
        )
        .bind(status.as_db_str())
        .bind(&note)
        .bind(conflict_id)
        .bind(tenant_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    #[tracing::instrument(skip(self, filter), fields(%tenant_id))]
    async fn batch_invalidate(
        &self,
        tenant_id: &str,
        filter: crate::BatchInvalidateFilter,
    ) -> Result<usize> {
        let mut conditions = vec!["tenant_id = $1".to_string()];
        let mut param_idx = 2u32;

        // Build dynamic WHERE clauses — we bind values positionally below
        if filter.domain.is_some() {
            conditions.push(format!("metadata->>'domain' = ${}", param_idx));
            param_idx += 1;
        }
        if filter.created_before.is_some() {
            conditions.push(format!("created_at < ${}", param_idx));
            param_idx += 1;
        }
        if filter.knowledge_type.is_some() {
            conditions.push(format!("metadata->>'knowledge_type' = ${}", param_idx));
            param_idx += 1;
        }
        if let Some(ref tags) = filter.tags {
            // Match claims where metadata->'tags' array overlaps with any of the given tags
            for _tag in tags {
                conditions.push(format!("metadata->'tags' @> ${}::jsonb", param_idx));
                param_idx += 1;
            }
        }

        let sql = format!(
            "UPDATE epistemic_claims SET epistemic_status = 'retracted', confidence = 0.0, updated_at = NOW() WHERE {} AND epistemic_status != 'retracted'",
            conditions.join(" AND ")
        );

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let mut query = sqlx::query(&sql).bind(tenant_id);

        if let Some(ref domain) = filter.domain {
            query = query.bind(domain);
        }
        if let Some(ref created_before) = filter.created_before {
            query = query.bind(created_before);
        }
        if let Some(ref knowledge_type) = filter.knowledge_type {
            query = query.bind(knowledge_type);
        }
        if let Some(ref tags) = filter.tags {
            for tag in tags {
                let json_val = format!("[\"{}\"]", tag);
                query = query.bind(json_val);
            }
        }

        let result = query
            .execute(tx.as_mut())
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }

    #[tracing::instrument(skip(self), fields(%old_id, %new_id, %tenant_id))]
    async fn supersede_claim(&self, old_id: Id, new_id: Id, tenant_id: &str) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        // Verify both claims belong to this tenant
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM epistemic_claims WHERE id = $1 AND tenant_id = $2",
        )
        .bind(new_id)
        .bind(tenant_id)
        .fetch_one(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?;

        if count.0 == 0 {
            return Err(CogKosError::NotFound(format!(
                "New claim {} not found for tenant {}",
                new_id, tenant_id
            )));
        }

        let rows_affected = sqlx::query(
            r#"
            UPDATE epistemic_claims
            SET superseded_by = $1,
                epistemic_status = 'superseded',
                confidence = 0.1,
                updated_at = NOW()
            WHERE id = $2 AND tenant_id = $3
            "#,
        )
        .bind(new_id)
        .bind(old_id)
        .bind(tenant_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| CogKosError::Database(e.to_string()))?
        .rows_affected();

        if rows_affected == 0 {
            return Err(CogKosError::NotFound(format!(
                "Old claim {} not found for tenant {}",
                old_id, tenant_id
            )));
        }

        tx.commit()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn list_unresolved_conflicts(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<ConflictRecord>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CogKosError::Database(e.to_string()))?;
        Self::set_tenant_context(tx.as_mut(), tenant_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, claim_a_id, claim_b_id, conflict_type, severity,
                description, detected_at, resolved_at, resolution, resolution_status, elevated_insight_id
            FROM conflict_records
            WHERE tenant_id = $1 AND resolution_status = 'open'
            ORDER BY detected_at DESC
            LIMIT $2
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

        rows.iter().map(row_to_conflict).collect()
    }
}

pub(crate) fn row_to_claim(row: &PgRow) -> Result<EpistemicClaim> {
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
    NodeType::from_db_str(s)
}

fn parse_epistemic_status(s: &str) -> EpistemicStatus {
    EpistemicStatus::from_db_str(s)
}

fn parse_consolidation_stage(s: &str) -> ConsolidationStage {
    ConsolidationStage::from_db_str(s)
}

fn parse_conflict_type(s: &str) -> ConflictType {
    ConflictType::from_db_str(s)
}

fn parse_resolution_status(s: &str) -> ResolutionStatus {
    ResolutionStatus::from_db_str(s)
}
