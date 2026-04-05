//! Authority Tier system for knowledge prioritization
//!
//! Derives a runtime authority level from existing EpistemicClaim fields
//! (knowledge_type, claimant, epistemic_status, provenance, metadata).
//! No new database columns required.

use crate::models::*;

/// Authority tier — higher value = higher authority.
/// Used for query ranking, decay modulation, and conflict resolution hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthorityTier {
    /// T5: working/episodic memory, external RSS feeds
    Ephemeral = 0,
    /// T4: Agent + Asserted (default for new claims)
    Observed = 1,
    /// T3: Corroborated or high confidence + frequent access
    Verified = 2,
    /// T2: Business knowledge or Human-uploaded documents
    Curated = 3,
    /// T1: Business + Human admin, or policy/SOP sources
    Canonical = 4,
}

impl AuthorityTier {
    /// Resolve tier from claim's existing fields (pure function, no DB access)
    pub fn resolve(claim: &EpistemicClaim) -> Self {
        let is_business = matches!(claim.knowledge_type, KnowledgeType::Business);
        let is_human_admin = matches!(
            &claim.claimant,
            Claimant::Human { role, .. }
                if role.eq_ignore_ascii_case("admin") || role.eq_ignore_ascii_case("owner")
        );
        let is_human = matches!(&claim.claimant, Claimant::Human { .. });
        let is_document = claim.provenance.source_type == "document"
            || claim.provenance.source_type == "upload"
            || claim.provenance.ingestion_method == "pipeline";
        let is_policy =
            claim.provenance.source_type == "policy" || claim.provenance.source_type == "sop";
        let is_corroborated = matches!(claim.epistemic_status, EpistemicStatus::Corroborated);
        let is_external = matches!(&claim.claimant, Claimant::ExternalPublic { .. });
        let is_ephemeral_layer = claim
            .metadata
            .get("memory_layer")
            .and_then(|v| v.as_str())
            .is_some_and(|l| l == "working" || l == "episodic");

        // T1: Business + admin/policy
        if is_business && (is_human_admin || is_policy) {
            return Self::Canonical;
        }
        // T2: Business knowledge, or Human + document upload
        if is_business || (is_human && is_document) {
            return Self::Curated;
        }
        // T5: ephemeral memory layer or external RSS
        if is_ephemeral_layer || (is_external && claim.provenance.source_type == "rss") {
            return Self::Ephemeral;
        }
        // T3: Corroborated, or high confidence + frequently accessed
        if is_corroborated || (claim.confidence >= 0.8 && claim.access_count >= 10) {
            return Self::Verified;
        }
        // T4: default
        Self::Observed
    }

    /// Score boost applied during query ranking (additive)
    pub fn score_boost(&self) -> f64 {
        match self {
            Self::Canonical => 0.3,
            Self::Curated => 0.2,
            Self::Verified => 0.1,
            Self::Observed => 0.0,
            Self::Ephemeral => -0.1,
        }
    }

    /// Multiplier applied to base decay lambda.
    /// 0.0 = no decay (Canonical), 2.0 = accelerated decay (Ephemeral).
    pub fn decay_multiplier(&self) -> f64 {
        match self {
            Self::Canonical => 0.0,
            Self::Curated => 0.2,
            Self::Verified => 0.5,
            Self::Observed => 1.0,
            Self::Ephemeral => 2.0,
        }
    }

    /// Numeric priority for conflict resolution (higher = takes precedence)
    pub fn conflict_priority(&self) -> u8 {
        *self as u8
    }

    /// Recommended durability value for newly ingested claims
    pub fn recommended_durability(&self) -> f64 {
        match self {
            Self::Canonical => 2.0,
            Self::Curated => 1.5,
            Self::Verified => 1.0,
            Self::Observed => 1.0,
            Self::Ephemeral => 0.5,
        }
    }

    /// String representation for metadata/logging
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Curated => "curated",
            Self::Verified => "verified",
            Self::Observed => "observed",
            Self::Ephemeral => "ephemeral",
        }
    }
}

impl std::fmt::Display for AuthorityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_claim() -> EpistemicClaim {
        EpistemicClaim::new(
            "test content",
            "test-tenant",
            NodeType::Entity,
            Claimant::Agent {
                agent_id: "agent-1".into(),
                model: "gpt-4o".into(),
            },
            AccessEnvelope::new("test-tenant"),
            ProvenanceRecord::new("src".into(), "experience".into(), "mcp_submit".into()),
        )
    }

    #[test]
    fn test_default_is_observed() {
        let claim = base_claim();
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Observed);
    }

    #[test]
    fn test_canonical_business_admin() {
        let mut claim = base_claim();
        claim.knowledge_type = KnowledgeType::Business;
        claim.claimant = Claimant::Human {
            user_id: "admin-1".into(),
            role: "admin".into(),
        };
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Canonical);
    }

    #[test]
    fn test_canonical_business_owner() {
        let mut claim = base_claim();
        claim.knowledge_type = KnowledgeType::Business;
        claim.claimant = Claimant::Human {
            user_id: "owner-1".into(),
            role: "Owner".into(),
        };
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Canonical);
    }

    #[test]
    fn test_canonical_business_policy() {
        let mut claim = base_claim();
        claim.knowledge_type = KnowledgeType::Business;
        claim.provenance.source_type = "policy".into();
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Canonical);
    }

    #[test]
    fn test_curated_business_agent() {
        let mut claim = base_claim();
        claim.knowledge_type = KnowledgeType::Business;
        // Agent claimant, not admin — should be Curated, not Canonical
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Curated);
    }

    #[test]
    fn test_curated_human_document() {
        let mut claim = base_claim();
        claim.claimant = Claimant::Human {
            user_id: "u1".into(),
            role: "analyst".into(),
        };
        claim.provenance.source_type = "document".into();
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Curated);
    }

    #[test]
    fn test_curated_human_pipeline() {
        let mut claim = base_claim();
        claim.claimant = Claimant::Human {
            user_id: "u1".into(),
            role: "user".into(),
        };
        claim.provenance.ingestion_method = "pipeline".into();
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Curated);
    }

    #[test]
    fn test_ephemeral_working_memory() {
        let mut claim = base_claim();
        claim.metadata.insert(
            "memory_layer".into(),
            serde_json::Value::String("working".into()),
        );
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Ephemeral);
    }

    #[test]
    fn test_ephemeral_episodic_memory() {
        let mut claim = base_claim();
        claim.metadata.insert(
            "memory_layer".into(),
            serde_json::Value::String("episodic".into()),
        );
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Ephemeral);
    }

    #[test]
    fn test_ephemeral_external_rss() {
        let mut claim = base_claim();
        claim.claimant = Claimant::ExternalPublic {
            source_name: "tech-feed".into(),
        };
        claim.provenance.source_type = "rss".into();
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Ephemeral);
    }

    #[test]
    fn test_verified_corroborated() {
        let mut claim = base_claim();
        claim.epistemic_status = EpistemicStatus::Corroborated;
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Verified);
    }

    #[test]
    fn test_verified_high_confidence_high_access() {
        let mut claim = base_claim();
        claim.confidence = 0.85;
        claim.access_count = 15;
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Verified);
    }

    #[test]
    fn test_high_confidence_low_access_stays_observed() {
        let mut claim = base_claim();
        claim.confidence = 0.9;
        claim.access_count = 5; // below threshold of 10
        assert_eq!(AuthorityTier::resolve(&claim), AuthorityTier::Observed);
    }

    #[test]
    fn test_score_boost_ordering() {
        assert!(AuthorityTier::Canonical.score_boost() > AuthorityTier::Curated.score_boost());
        assert!(AuthorityTier::Curated.score_boost() > AuthorityTier::Verified.score_boost());
        assert!(AuthorityTier::Verified.score_boost() > AuthorityTier::Observed.score_boost());
        assert!(AuthorityTier::Observed.score_boost() > AuthorityTier::Ephemeral.score_boost());
    }

    #[test]
    fn test_decay_multiplier_ordering() {
        assert_eq!(AuthorityTier::Canonical.decay_multiplier(), 0.0);
        assert!(
            AuthorityTier::Curated.decay_multiplier() < AuthorityTier::Observed.decay_multiplier()
        );
        assert!(
            AuthorityTier::Ephemeral.decay_multiplier()
                > AuthorityTier::Observed.decay_multiplier()
        );
    }

    #[test]
    fn test_conflict_priority_ordering() {
        assert!(
            AuthorityTier::Canonical.conflict_priority()
                > AuthorityTier::Ephemeral.conflict_priority()
        );
    }

    #[test]
    fn test_as_str_round_trip() {
        let tiers = [
            AuthorityTier::Canonical,
            AuthorityTier::Curated,
            AuthorityTier::Verified,
            AuthorityTier::Observed,
            AuthorityTier::Ephemeral,
        ];
        for tier in tiers {
            assert!(!tier.as_str().is_empty());
            assert_eq!(format!("{}", tier), tier.as_str());
        }
    }
}
