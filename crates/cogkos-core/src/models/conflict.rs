use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Conflict record between two claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub claim_a_id: Uuid,
    pub claim_b_id: Uuid,
    pub conflict_type: ConflictType,
    pub severity: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub detected_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub resolution_status: ResolutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevated_insight_id: Option<Uuid>,
}

impl ConflictRecord {
    /// Create a new conflict record
    pub fn new(
        tenant_id: impl Into<String>,
        claim_a_id: Uuid,
        claim_b_id: Uuid,
        conflict_type: ConflictType,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.into(),
            claim_a_id,
            claim_b_id,
            conflict_type,
            severity: 0.5,
            description: None,
            detected_at: chrono::Utc::now(),
            resolved_at: None,
            resolution_status: ResolutionStatus::Open,
            resolution: None,
            resolution_note: None,
            elevated_insight_id: None,
        }
    }

    /// Mark as elevated to insight
    pub fn elevate(&mut self, insight_id: Uuid) {
        self.resolution_status = ResolutionStatus::Elevated;
        self.elevated_insight_id = Some(insight_id);
    }

    /// Mark as dismissed
    pub fn dismiss(&mut self, note: impl Into<String>) {
        self.resolution_status = ResolutionStatus::Dismissed;
        self.resolution_note = Some(note.into());
    }

    /// Mark as accepted
    pub fn accept(&mut self, note: impl Into<String>) {
        self.resolution_status = ResolutionStatus::Accepted;
        self.resolution_note = Some(note.into());
    }
}

/// Type of conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConflictType {
    DirectContradiction,
    ContextDependent,
    TemporalShift,
    TemporalInconsistency,
    SourceDisagreement,
    ConfidenceMismatch,
    ContextualDifference,
}

/// Resolution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ResolutionStatus {
    Open,
    Elevated,
    Dismissed,
    Accepted,
}
