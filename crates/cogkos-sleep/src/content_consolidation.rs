//! Content consolidation engine — episodic turns → semantic facts
//!
//! Groups episodic claims by speaker entity, extracts key facts using
//! keyword patterns (no LLM), and produces structured facts that can be
//! elevated to semantic-layer claims by the scheduler task.

use cogkos_core::models::EpistemicClaim;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Fact categories & extracted fact
// ---------------------------------------------------------------------------

/// Category of an extracted fact
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactCategory {
    Activity,
    Preference,
    Relationship,
    Career,
    Identity,
    Location,
    Event,
}

/// A fact extracted from a group of episodic claims
#[derive(Debug, Clone)]
pub struct ExtractedFact {
    pub category: FactCategory,
    pub content: String,
    pub source_claim_ids: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Keyword lists
// ---------------------------------------------------------------------------

const ACTIVITY_KEYWORDS: &[&str] = &[
    "pottery",
    "painting",
    "camping",
    "swimming",
    "running",
    "cooking",
    "hiking",
    "reading",
    "writing",
    "singing",
    "dancing",
    "cycling",
    "yoga",
    "meditation",
    "gardening",
    "fishing",
    "surfing",
    "skiing",
    "climbing",
    "photography",
    "volunteering",
    "tutoring",
];

const RELATIONSHIP_KEYWORDS: &[&str] = &[
    "husband", "wife", "kids", "children", "son", "daughter", "mother", "father", "friend",
    "partner", "sibling", "brother", "sister",
];

const CAREER_KEYWORDS: &[&str] = &[
    "career",
    "job",
    "work",
    "profession",
    "counseling",
    "teaching",
    "engineering",
    "research",
    "study",
    "education",
    "university",
    "degree",
];

// ---------------------------------------------------------------------------
// Speaker grouping
// ---------------------------------------------------------------------------

/// Group episodic claims by the speaker name extracted from "Speaker: content" format.
///
/// Keys are lowercased speaker names. Claims without recognisable speaker prefix
/// are silently skipped.
pub fn group_claims_by_speaker(claims: &[EpistemicClaim]) -> HashMap<String, Vec<&EpistemicClaim>> {
    let mut groups: HashMap<String, Vec<&EpistemicClaim>> = HashMap::new();
    for claim in claims {
        if let Some(speaker) = extract_speaker(&claim.content) {
            groups.entry(speaker).or_default().push(claim);
        }
    }
    groups
}

/// Extract speaker name from "Speaker: content" format.
fn extract_speaker(content: &str) -> Option<String> {
    let colon_pos = content.find(':')?;
    let speaker = content[..colon_pos].trim();
    if speaker.is_empty() || speaker.len() > 50 {
        return None;
    }
    // Must start with uppercase letter (proper name)
    if !speaker.chars().next()?.is_uppercase() {
        return None;
    }
    Some(speaker.to_lowercase())
}

// ---------------------------------------------------------------------------
// Fact extraction
// ---------------------------------------------------------------------------

/// Extract consolidated facts from a speaker's claims using keyword matching.
pub fn extract_speaker_facts(speaker: &str, claims: &[&EpistemicClaim]) -> Vec<ExtractedFact> {
    let mut facts: Vec<ExtractedFact> = Vec::new();

    // 1. Activity extraction
    let mut activities: Vec<(String, Uuid)> = Vec::new();
    for claim in claims {
        let lower = claim.content.to_lowercase();
        for &kw in ACTIVITY_KEYWORDS {
            if lower.contains(kw) {
                activities.push((kw.to_string(), claim.id));
            }
        }
    }
    if !activities.is_empty() {
        let names: Vec<String> = activities
            .iter()
            .map(|(a, _)| a.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let ids: Vec<Uuid> = activities
            .iter()
            .map(|(_, id)| *id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mut sorted_names = names;
        sorted_names.sort();
        facts.push(ExtractedFact {
            category: FactCategory::Activity,
            content: format!(
                "{}'s activities include {}",
                capitalize(speaker),
                sorted_names.join(", ")
            ),
            source_claim_ids: ids,
        });
    }

    // 2. Relationship extraction
    let mut rels: Vec<(String, Uuid)> = Vec::new();
    for claim in claims {
        let lower = claim.content.to_lowercase();
        for &kw in RELATIONSHIP_KEYWORDS {
            if lower.contains(kw) {
                rels.push((kw.to_string(), claim.id));
            }
        }
    }
    if !rels.is_empty() {
        let names: Vec<String> = rels
            .iter()
            .map(|(r, _)| r.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let ids: Vec<Uuid> = rels
            .iter()
            .map(|(_, id)| *id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mut sorted_names = names;
        sorted_names.sort();
        facts.push(ExtractedFact {
            category: FactCategory::Relationship,
            content: format!(
                "{} has mentioned: {}",
                capitalize(speaker),
                sorted_names.join(", ")
            ),
            source_claim_ids: ids,
        });
    }

    // 3. Career extraction
    let mut careers: Vec<(String, Uuid)> = Vec::new();
    for claim in claims {
        let lower = claim.content.to_lowercase();
        for &kw in CAREER_KEYWORDS {
            if lower.contains(kw) {
                // Extract the sentence containing the keyword
                for sentence in claim.content.split('.') {
                    if sentence.to_lowercase().contains(kw) && sentence.len() > 15 {
                        careers.push((sentence.trim().to_string(), claim.id));
                        break;
                    }
                }
            }
        }
    }
    if !careers.is_empty() {
        let ids: Vec<Uuid> = careers
            .iter()
            .map(|(_, id)| *id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let best = careers.last().map(|(c, _)| c.clone()).unwrap_or_default();
        facts.push(ExtractedFact {
            category: FactCategory::Career,
            content: format!("{} — career: {}", capitalize(speaker), best),
            source_claim_ids: ids,
        });
    }

    facts
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cogkos_core::models::{AccessEnvelope, Claimant, NodeType, ProvenanceRecord};

    fn make_claim(content: &str, speaker: &str) -> EpistemicClaim {
        let full = format!("{}: {}", speaker, content);
        EpistemicClaim::new(
            full,
            "test",
            NodeType::Entity,
            Claimant::System,
            AccessEnvelope::new("test"),
            ProvenanceRecord::new("test".to_string(), "test".to_string(), "test".to_string()),
        )
    }

    #[test]
    fn test_group_by_speaker() {
        let claims = vec![
            make_claim("I love pottery", "Caroline"),
            make_claim("That sounds great!", "Melanie"),
            make_claim("I also went camping", "Caroline"),
            make_claim("My kids love nature", "Melanie"),
        ];
        let groups = group_claims_by_speaker(&claims);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups.get("caroline").unwrap().len(), 2);
        assert_eq!(groups.get("melanie").unwrap().len(), 2);
    }

    #[test]
    fn test_group_ignores_non_speaker_format() {
        let mut claim = make_claim("no speaker here", "X");
        claim.content = "no colon here".to_string();
        let claims = vec![claim];
        let groups = group_claims_by_speaker(&claims);
        assert!(groups.is_empty());
    }

    #[test]
    fn test_group_ignores_lowercase_prefix() {
        let mut claim = make_claim("test", "X");
        claim.content = "lowercase: something".to_string();
        let claims = vec![claim];
        let groups = group_claims_by_speaker(&claims);
        assert!(groups.is_empty());
    }

    #[test]
    fn test_extract_speaker_facts_activities() {
        let claims = vec![
            make_claim("I love pottery and painting", "Melanie"),
            make_claim("We went camping last weekend at the beach", "Melanie"),
            make_claim("I signed up for a swimming class", "Melanie"),
            make_claim("My kids are doing great in school", "Melanie"),
        ];
        let refs: Vec<&EpistemicClaim> = claims.iter().collect();
        let facts = extract_speaker_facts("melanie", &refs);
        assert!(facts.iter().any(|f| f.category == FactCategory::Activity));
        let activity = facts
            .iter()
            .find(|f| f.category == FactCategory::Activity)
            .unwrap();
        assert!(activity.content.contains("pottery"));
        assert!(activity.content.contains("painting"));
        assert!(activity.content.contains("camping"));
        assert!(activity.content.contains("swimming"));
    }

    #[test]
    fn test_extract_speaker_facts_relationships() {
        let claims = vec![
            make_claim("My kids love nature", "Melanie"),
            make_claim("My husband works late", "Melanie"),
        ];
        let refs: Vec<&EpistemicClaim> = claims.iter().collect();
        let facts = extract_speaker_facts("melanie", &refs);
        assert!(
            facts
                .iter()
                .any(|f| f.category == FactCategory::Relationship)
        );
        let rel = facts
            .iter()
            .find(|f| f.category == FactCategory::Relationship)
            .unwrap();
        assert!(rel.content.contains("kids"));
        assert!(rel.content.contains("husband"));
    }

    #[test]
    fn test_extract_speaker_facts_career() {
        let claims = vec![make_claim(
            "I've been working in counseling for years now",
            "Melanie",
        )];
        let refs: Vec<&EpistemicClaim> = claims.iter().collect();
        let facts = extract_speaker_facts("melanie", &refs);
        assert!(facts.iter().any(|f| f.category == FactCategory::Career));
    }

    #[test]
    fn test_extract_no_facts_from_empty() {
        let claims: Vec<EpistemicClaim> = vec![];
        let refs: Vec<&EpistemicClaim> = claims.iter().collect();
        let facts = extract_speaker_facts("nobody", &refs);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_source_claim_ids_tracked() {
        let claims = vec![
            make_claim("I love pottery", "Caroline"),
            make_claim("I also enjoy painting", "Caroline"),
        ];
        let refs: Vec<&EpistemicClaim> = claims.iter().collect();
        let facts = extract_speaker_facts("caroline", &refs);
        let activity = facts
            .iter()
            .find(|f| f.category == FactCategory::Activity)
            .unwrap();
        // Both claims contribute to the activity fact
        assert!(activity.source_claim_ids.len() >= 2);
    }
}
