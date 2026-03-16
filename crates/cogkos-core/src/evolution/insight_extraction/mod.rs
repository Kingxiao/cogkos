//! Insight extraction from conflicts
//! Phase 3: Convert conflicts into higher-level insights

pub mod extractor;

pub use extractor::*;

/// Insight extracted from conflict analysis
#[derive(Debug, Clone)]
pub struct ExtractedInsight {
    pub id: uuid::Uuid,
    pub content: String,
    pub confidence: f64,
    pub source_conflicts: Vec<uuid::Uuid>,
    pub insight_type: InsightType,
    pub supporting_claims: Vec<uuid::Uuid>,
    pub key_entities: Vec<String>,
    pub temporal_context: Option<TemporalContext>,
}

/// Type of insight extracted
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsightType {
    /// Different sources have different data
    SourceDiscrepancy,
    /// Conditions or context explain the difference
    ContextualQualifier,
    /// Change over time
    TemporalEvolution,
    /// Domain boundary or limitation
    DomainBoundary,
    /// Uncertainty or confidence issue
    UncertaintyIndicator,
    /// Emerging pattern
    EmergingPattern,
}

/// Temporal context for insights
#[derive(Debug, Clone)]
pub struct TemporalContext {
    pub valid_from: Option<chrono::DateTime<chrono::Utc>>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    pub trend_direction: TrendDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendDirection {
    Increasing,
    Decreasing,
    Stable,
    Fluctuating,
    Unknown,
}

/// Configuration for insight extraction
#[derive(Clone, Debug)]
pub struct InsightExtractionConfig {
    /// Minimum number of conflicts to trigger analysis
    pub min_conflicts: usize,
    /// Minimum conflict density threshold
    pub min_conflict_density: f64,
    /// Confidence threshold for extracted insights
    pub min_insight_confidence: f64,
    /// Enable LLM-based insight extraction
    pub use_llm_extraction: bool,
}

impl Default for InsightExtractionConfig {
    fn default() -> Self {
        Self {
            min_conflicts: 3,
            min_conflict_density: 0.1,
            min_insight_confidence: 0.6,
            use_llm_extraction: false,
        }
    }
}
