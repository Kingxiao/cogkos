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

impl NodeType {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            NodeType::Entity => "entity",
            NodeType::Relation => "relation",
            NodeType::Event => "event",
            NodeType::Attribute => "attribute",
            NodeType::Prediction => "prediction",
            NodeType::Insight => "insight",
            NodeType::File => "file",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
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
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
    }
}

/// Type of knowledge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KnowledgeType {
    Business, // Admin-maintained, version-replace updates, shared across all Agents
    #[default]
    Experiential, // Agent-contributed, progressively aggregated, partitioned by role/customer
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

/// Memory layer (cognitive architecture: Atkinson-Shiffrin three-store model)
///
/// Stored in `metadata.memory_layer` for backward compatibility.
/// This enum provides type-safe access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayer {
    /// Active context — λ=0.5, half-life ~1.4h, session-scoped
    Working,
    /// Event memories — λ=0.05, half-life ~14h, session summaries
    Episodic,
    /// Long-term knowledge — λ=0.01, half-life ~3d (default)
    Semantic,
}

impl MemoryLayer {
    /// Decay rate (lambda) for this memory layer
    pub fn lambda(&self) -> f64 {
        match self {
            MemoryLayer::Working => 0.5,
            MemoryLayer::Episodic => 0.05,
            MemoryLayer::Semantic => 0.01,
        }
    }

    /// Hard TTL in hours
    pub fn max_age_hours(&self) -> f64 {
        match self {
            MemoryLayer::Working => 8.0,
            MemoryLayer::Episodic => 168.0, // 7 days
            MemoryLayer::Semantic => 720.0, // 30 days
        }
    }

    /// Parse from claim metadata
    pub fn from_metadata(metadata: &serde_json::Map<String, serde_json::Value>) -> Self {
        metadata
            .get("memory_layer")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "working" => MemoryLayer::Working,
                "episodic" => MemoryLayer::Episodic,
                _ => MemoryLayer::Semantic,
            })
            .unwrap_or(MemoryLayer::Semantic)
    }
}

impl Default for MemoryLayer {
    fn default() -> Self {
        MemoryLayer::Semantic
    }
}

impl std::fmt::Display for MemoryLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryLayer::Working => write!(f, "working"),
            MemoryLayer::Episodic => write!(f, "episodic"),
            MemoryLayer::Semantic => write!(f, "semantic"),
        }
    }
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

impl ConsolidationStage {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            ConsolidationStage::FastTrack => "fast_track",
            ConsolidationStage::PendingAggregation => "pending_aggregation",
            ConsolidationStage::Consolidated => "consolidated",
            ConsolidationStage::Insight => "insight",
            ConsolidationStage::Archived => "archived",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "fast_track" | "fasttrack" => ConsolidationStage::FastTrack,
            "pending_aggregation" | "pendingaggregation" => ConsolidationStage::PendingAggregation,
            "consolidated" => ConsolidationStage::Consolidated,
            "insight" => ConsolidationStage::Insight,
            "archived" => ConsolidationStage::Archived,
            _ => ConsolidationStage::FastTrack,
        }
    }
}

impl std::fmt::Display for ConsolidationStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
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

impl EpistemicStatus {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            EpistemicStatus::Asserted => "asserted",
            EpistemicStatus::Corroborated => "corroborated",
            EpistemicStatus::Contested => "contested",
            EpistemicStatus::Retracted => "retracted",
            EpistemicStatus::Superseded => "superseded",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "asserted" => EpistemicStatus::Asserted,
            "corroborated" => EpistemicStatus::Corroborated,
            "contested" => EpistemicStatus::Contested,
            "retracted" => EpistemicStatus::Retracted,
            "superseded" => EpistemicStatus::Superseded,
            _ => EpistemicStatus::Asserted,
        }
    }
}

impl std::fmt::Display for EpistemicStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
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

    /// Get the memory layer of this claim
    pub fn memory_layer(&self) -> MemoryLayer {
        MemoryLayer::from_metadata(&self.metadata)
    }

    /// Get session_id from metadata (if working/episodic memory)
    pub fn session_id(&self) -> Option<&str> {
        self.metadata.get("session_id").and_then(|v| v.as_str())
    }

    /// Get rehearsal count from metadata (working memory)
    pub fn rehearsal_count(&self) -> u64 {
        self.metadata
            .get("rehearsal_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }

    /// Increment rehearsal count (called on every recall)
    pub fn record_rehearsal(&mut self) {
        let count = self.rehearsal_count() + 1;
        self.metadata.insert(
            "rehearsal_count".to_string(),
            serde_json::Value::Number(serde_json::Number::from(count)),
        );
        self.record_access(self.memory_layer().lambda() * 0.4); // rehearsal boosts activation
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
