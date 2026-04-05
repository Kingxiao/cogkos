//! Federation Layer - Anonymous insight export/import and cross-instance protocol
//!
//! Implements the federation protocol for secure insight sharing between
//! independent CogKOS instances while preserving privacy.

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use cogkos_core::{ConsolidationStage, EpistemicClaim, NodeType};

/// Federation protocol errors
#[derive(Error, Debug)]
pub enum FederationProtocolError {
    #[error("Export failed: {0}")]
    ExportFailed(String),
    #[error("Import failed: {0}")]
    ImportFailed(String),
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    HttpError(String),
}

pub type Result<T> = std::result::Result<T, FederationProtocolError>;

/// Anonymous insight - privacy-preserving knowledge export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnonymousInsight {
    pub id: Uuid,
    pub content_hash: String,
    pub anonymized_content: String,
    pub node_type: NodeType,
    pub confidence: f64,
    pub consolidation_stage: ConsolidationStage,
    /// Tenant-agnostic domain classification
    pub domain_tags: Vec<String>,
    /// Time anonymized
    pub time_bucket: TimeBucket,
    /// Source instance identifier (hashed)
    pub source_instance_hash: String,
    /// Statistical properties (no identifying info)
    pub statistics: InsightStatistics,
    /// Validation metadata
    pub validation: ValidationMetadata,
}

/// Time bucketing for anonymity
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TimeBucket {
    Hour,
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl TimeBucket {
    /// Convert timestamp to bucket
    pub fn from_timestamp(timestamp: DateTime<Utc>, bucket: Self) -> String {
        match bucket {
            TimeBucket::Hour => timestamp.format("%Y-%m-%d-%H").to_string(),
            TimeBucket::Day => timestamp.format("%Y-%m-%d").to_string(),
            TimeBucket::Week => {
                let week = timestamp.iso_week();
                format!("{}-W{:02}", timestamp.year(), week.week())
            }
            TimeBucket::Month => timestamp.format("%Y-%m").to_string(),
            TimeBucket::Quarter => {
                let quarter = (timestamp.month() - 1) / 3 + 1;
                format!("{}-Q{}", timestamp.year(), quarter)
            }
            TimeBucket::Year => timestamp.format("%Y").to_string(),
        }
    }
}

/// Statistical properties of an insight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightStatistics {
    /// Number of supporting claims
    pub support_count: u32,
    /// Number of conflicting claims
    pub conflict_count: u32,
    /// Source diversity score
    pub source_diversity: f64,
    /// Temporal spread
    pub temporal_range_days: u32,
    /// Activation score (anonymized)
    pub normalized_activation: f64,
}

/// Validation metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationMetadata {
    /// Number of successful predictions
    pub successful_predictions: u32,
    /// Number of failed predictions
    pub failed_predictions: u32,
    /// Last validation timestamp (bucketed)
    pub last_validated_bucket: String,
    /// Cross-instance corroborations
    pub corroboration_count: u32,
}

/// Anonymization configuration
#[derive(Debug, Clone)]
pub struct AnonymizationConfig {
    /// Time bucket granularity
    pub time_bucket: TimeBucket,
    /// Minimum confidence threshold for export
    pub min_confidence: f64,
    /// Required consolidation stage
    pub min_stage: ConsolidationStage,
    /// Anonymize entity names
    pub anonymize_entities: bool,
    /// Remove geographic references
    pub remove_locations: bool,
    /// Hash algorithm for IDs
    pub hash_algorithm: HashAlgorithm,
}

impl Default for AnonymizationConfig {
    fn default() -> Self {
        Self {
            time_bucket: TimeBucket::Week,
            min_confidence: 0.7,
            min_stage: ConsolidationStage::Consolidated,
            anonymize_entities: true,
            remove_locations: true,
            hash_algorithm: HashAlgorithm::Sha256,
        }
    }
}

/// Hash algorithm selection
#[derive(Debug, Clone, Copy)]
pub enum HashAlgorithm {
    Sha256,
    Blake3,
}

/// Insight anonymizer
pub struct InsightAnonymizer {
    config: AnonymizationConfig,
    entity_patterns: Vec<regex::Regex>,
    location_patterns: Vec<regex::Regex>,
}

impl InsightAnonymizer {
    pub fn new(config: AnonymizationConfig) -> Self {
        let entity_patterns = vec![
            regex::Regex::new(r"\b[A-Z][a-z]+ (Inc|Corp|Ltd|LLC|Company)\b").expect("valid regex"),
            regex::Regex::new(r"\b[A-Z][a-z]+ [A-Z][a-z]+\b").expect("valid regex"),
        ];

        let location_patterns =
            vec![regex::Regex::new(r"\b(?:in|at|near) [A-Z][a-zA-Z\s]+\b").expect("valid regex")];

        Self {
            config,
            entity_patterns,
            location_patterns,
        }
    }

    /// Anonymize a single claim
    pub fn anonymize(&self, claim: &EpistemicClaim, instance_id: &str) -> Result<AnonymousInsight> {
        // Check thresholds
        if claim.confidence < self.config.min_confidence {
            return Err(FederationProtocolError::ExportFailed(format!(
                "Confidence {} below threshold {}",
                claim.confidence, self.config.min_confidence
            )));
        }

        if (claim.consolidation_stage as i32) < (self.config.min_stage as i32) {
            return Err(FederationProtocolError::ExportFailed(
                "Consolidation stage below threshold".to_string(),
            ));
        }

        // Anonymize content
        let anonymized_content = self.anonymize_content(&claim.content);

        // Generate content hash
        let content_hash = self.hash_content(&anonymized_content);

        // Generate source instance hash
        let source_instance_hash = self.hash_instance(instance_id);

        // Calculate statistics
        let statistics = InsightStatistics {
            support_count: 1, // Would be calculated from claim relationships
            conflict_count: 0,
            source_diversity: 0.5, // Calculated from source types
            temporal_range_days: 30,
            normalized_activation: claim.activation_weight,
        };

        let validation = ValidationMetadata {
            successful_predictions: 0,
            failed_predictions: 0,
            last_validated_bucket: TimeBucket::from_timestamp(Utc::now(), self.config.time_bucket),
            corroboration_count: 0,
        };

        Ok(AnonymousInsight {
            id: Uuid::new_v4(),
            content_hash,
            anonymized_content,
            node_type: claim.node_type,
            confidence: claim.confidence,
            consolidation_stage: claim.consolidation_stage,
            domain_tags: claim
                .metadata
                .get("domain")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            time_bucket: self.config.time_bucket,
            source_instance_hash,
            statistics,
            validation,
        })
    }

    /// Anonymize content by removing/replacing identifying information
    pub(crate) fn anonymize_content(&self, content: &str) -> String {
        let mut result = content.to_string();

        if self.config.anonymize_entities {
            for pattern in &self.entity_patterns {
                result = pattern.replace_all(&result, "[ENTITY]").to_string();
            }
        }

        if self.config.remove_locations {
            for pattern in &self.location_patterns {
                result = pattern.replace_all(&result, "[LOCATION]").to_string();
            }
        }

        result
    }

    /// Hash content for deduplication
    fn hash_content(&self, content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Hash instance ID
    fn hash_instance(&self, instance_id: &str) -> String {
        self.hash_content(instance_id)
    }

    /// Batch anonymize claims
    pub fn anonymize_batch(
        &self,
        claims: &[EpistemicClaim],
        instance_id: &str,
    ) -> Vec<AnonymousInsight> {
        claims
            .iter()
            .filter_map(|claim| self.anonymize(claim, instance_id).ok())
            .collect()
    }
}
