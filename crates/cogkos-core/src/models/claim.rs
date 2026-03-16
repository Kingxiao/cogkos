use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Type of knowledge node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum NodeType {
    Entity,
    Relation,
    Event,
    Attribute,
    Prediction,
    Insight,
    File,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Entity => write!(f, "Entity"),
            NodeType::Relation => write!(f, "Relation"),
            NodeType::Event => write!(f, "Event"),
            NodeType::Attribute => write!(f, "Attribute"),
            NodeType::Prediction => write!(f, "Prediction"),
            NodeType::Insight => write!(f, "Insight"),
            NodeType::File => write!(f, "File"),
        }
    }
}

/// Type of knowledge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KnowledgeType {
    Business, // 管理员维护，版本替换式更新，全 Agent 共享
    #[default]
    Experiential, // Agent 贡献，渐进聚合，按角色/客户分区
}

impl std::fmt::Display for KnowledgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnowledgeType::Business => write!(f, "Business"),
            KnowledgeType::Experiential => write!(f, "Experiential"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRef {
    pub entity_type: String, // "customer" | "product" | "order"
    pub entity_id: String,   // "customer-001"
}

/// Who made the claim
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Claimant {
    Human { user_id: String, role: String },
    Agent { agent_id: String, model: String },
    System,
    ExternalPublic { source_name: String },
}

/// Stage of knowledge consolidation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConsolidationStage {
    FastTrack,
    PendingAggregation,
    Consolidated,
    Insight,
    Archived,
}

impl std::fmt::Display for ConsolidationStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsolidationStage::FastTrack => write!(f, "FastTrack"),
            ConsolidationStage::PendingAggregation => write!(f, "PendingAggregation"),
            ConsolidationStage::Consolidated => write!(f, "Consolidated"),
            ConsolidationStage::Insight => write!(f, "Insight"),
            ConsolidationStage::Archived => write!(f, "Archived"),
        }
    }
}

/// Epistemic status of a claim
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum EpistemicStatus {
    Asserted,
    Corroborated,
    Contested,
    Retracted,
    Superseded,
}

impl std::fmt::Display for EpistemicStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EpistemicStatus::Asserted => write!(f, "Asserted"),
            EpistemicStatus::Corroborated => write!(f, "Corroborated"),
            EpistemicStatus::Contested => write!(f, "Contested"),
            EpistemicStatus::Retracted => write!(f, "Retracted"),
            EpistemicStatus::Superseded => write!(f, "Superseded"),
        }
    }
}

/// Source provenance record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub source_id: String,
    pub source_type: String,
    pub ingestion_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_url: Option<String>,
    pub audit_hash: String,
}

impl ProvenanceRecord {
    pub fn new(source_id: String, source_type: String, ingestion_method: String) -> Self {
        Self {
            source_id,
            source_type,
            ingestion_method,
            original_url: None,
            audit_hash: String::new(),
        }
    }
}

/// Core knowledge atom - EpistemicClaim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpistemicClaim {
    pub id: Uuid,
    pub tenant_id: String,
    pub content: String,
    pub node_type: NodeType,
    pub knowledge_type: KnowledgeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
    pub claimant: Claimant,

    // Status
    pub epistemic_status: EpistemicStatus,
    pub confidence: f64,

    // Lifecycle
    pub consolidation_stage: ConsolidationStage,
    pub version: u32,
    pub durability: f64,

    // Activation (S3: read equals write)
    pub activation_weight: f64,
    pub access_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<chrono::DateTime<chrono::Utc>>,

    // Bitemporal
    pub t_valid_start: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t_valid_end: Option<chrono::DateTime<chrono::Utc>>,
    pub t_known: chrono::DateTime<chrono::Utc>,

    // Access control
    pub access_envelope: super::AccessEnvelope,

    // Provenance
    pub provenance: ProvenanceRecord,

    // Vector reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_id: Option<Uuid>,

    // Prediction tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_prediction_error: Option<f64>,

    // Evolution tracking
    #[serde(default)]
    pub derived_from: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<Uuid>,
    #[serde(default)]
    pub entity_refs: Vec<EntityRef>,
    #[serde(default)]
    pub needs_revalidation: bool,

    // Timestamps
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,

    // Metadata
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl EpistemicClaim {
    /// Create a new claim with default values
    pub fn new(
        content: impl Into<String>,
        tenant_id: impl Into<String>,
        node_type: NodeType,
        claimant: Claimant,
        access_envelope: super::AccessEnvelope,
        provenance: ProvenanceRecord,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.into(),
            content: content.into(),
            node_type,
            knowledge_type: KnowledgeType::Experiential,
            structured_content: None,
            claimant,
            epistemic_status: EpistemicStatus::Asserted,
            confidence: 0.5,
            consolidation_stage: ConsolidationStage::FastTrack,
            version: 1,
            durability: 1.0,
            activation_weight: 0.5,
            access_count: 0,
            last_accessed: None,
            t_valid_start: now,
            t_valid_end: None,
            t_known: now,
            access_envelope,
            provenance,
            vector_id: None,
            last_prediction_error: None,
            derived_from: Vec::new(),
            superseded_by: None,
            entity_refs: Vec::new(),
            needs_revalidation: false,
            created_at: now,
            updated_at: now,
            metadata: serde_json::Map::new(),
        }
    }

    /// Update activation on access (S3: read equals write)
    pub fn record_access(&mut self, delta: f64) {
        self.activation_weight = (self.activation_weight + delta).min(1.0);
        self.access_count += 1;
        self.last_accessed = Some(chrono::Utc::now());
    }

    /// Check if claim is currently valid
    pub fn is_valid(&self) -> bool {
        let now = chrono::Utc::now();
        self.t_valid_start <= now
            && self.t_valid_end.is_none_or(|end| now < end)
            && !matches!(
                self.epistemic_status,
                EpistemicStatus::Retracted | EpistemicStatus::Superseded
            )
    }
}
// TODO #197: Move durability, vector_id, last_prediction_error, needs_revalidation to metadata
