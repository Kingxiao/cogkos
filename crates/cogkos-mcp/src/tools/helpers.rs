//! Pure helper functions for MCP tools

use cogkos_core::models::*;
use std::hash::{Hash, Hasher};

/// Generate a pseudo-random vector from query string for fallback
pub(crate) fn generate_query_vector(query: &str) -> Vec<f32> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    query.hash(&mut hasher);
    let hash = hasher.finish();
    // MiniMax embo-01 returns 1536 dimensions
    (0..1536)
        .map(|i| {
            let h = hash.wrapping_add(i as u64);
            ((h % 1000) as f32 / 1000.0 - 0.5) * 0.1
        })
        .collect()
}

/// Extract domain from content using classification tags or keyword analysis
pub(crate) fn extract_domain(content: &str) -> String {
    let tag_patterns = [
        "cs.AI", "cs.CL", "cs.CR", "cs.CV", "cs.LG", "cs.NE", "cs.RO", "cs.SE", "cs.SY", "math.CO",
        "math.LO", "physics", "q-bio", "q-fin", "stat.ML", "econ", "eess", "astro-ph", "cond-mat",
        "hep-th", "nlin", "quant-ph",
    ];

    for tag in &tag_patterns {
        let pattern = format!("[{}]", tag);
        if content.contains(&pattern) {
            return tag.replace('.', "_");
        }
    }

    let content_lower = content.to_lowercase();
    for tag in &tag_patterns {
        let tag_lower = tag.to_lowercase();
        if content_lower.contains(&tag_lower) {
            return tag.replace('.', "_");
        }
    }

    let domain_keywords = [
        (
            "cs",
            vec![
                "algorithm", "software", "programming", "computer", "machine learning",
                "neural", "deep learning", "ai", "artificial intelligence", "nlp",
                "natural language", "computer vision", "robotics", "database", "network",
                "operating system", "compiler", "distributed", "cloud", "security",
                "软件", "编程", "算法", "计算机", "机器学习", "人工智能", "神经网络",
            ],
        ),
        (
            "math",
            vec![
                "mathematics", "theorem", "proof", "algebra", "geometry", "calculus",
                "topology", "analysis", "combinatorics", "number theory", "probability",
                "数学", "定理", "证明", "代数", "几何", "微积分", "拓扑",
            ],
        ),
        (
            "physics",
            vec![
                "physics", "quantum", "particle", "thermodynamics", "relativity",
                "electromagnetism", "mechanics", "cosmology", "astrophysics",
                "物理", "量子", "粒子", "热力学", "相对论", "力学",
            ],
        ),
        (
            "bio",
            vec![
                "biology", "biochemistry", "genetics", "molecular", "cell", "protein",
                "dna", "rna", "organism", "ecology", "evolution", "neuroscience",
                "生物", "生化", "基因", "分子", "细胞", "生态", "进化",
            ],
        ),
        (
            "medicine",
            vec![
                "medicine", "clinical", "drug", "therapy", "diagnosis", "patient",
                "disease", "treatment", "pharmaceutical", "vaccine",
                "医学", "医疗", "临床", "药物", "治疗", "诊断",
            ],
        ),
        (
            "finance",
            vec![
                "finance", "investment", "stock", "market", "trading", "portfolio",
                "risk", "asset", "pricing", "derivative", "banking", "economy",
                "金融", "投资", "股票", "市场", "银行", "经济", "风险",
            ],
        ),
        (
            "law",
            vec![
                "law", "legal", "court", "regulation", "contract", "compliance",
                "legislation", "patent", "copyright", "jurisdiction",
                "法律", "法规", "合规", "合同",
            ],
        ),
        (
            "psychology",
            vec![
                "psychology", "cognitive", "behavior", "mental", "emotion",
                "neuroscience", "therapy", "counseling", "psychiatric",
                "心理", "认知", "行为", "精神",
            ],
        ),
        (
            "education",
            vec![
                "education", "teaching", "learning", "curriculum", "student",
                "pedagogy", "classroom", "instruction",
                "教育", "教学", "学习", "学校", "学生",
            ],
        ),
        (
            "marketing",
            vec![
                "marketing", "brand", "advertising", "customer", "social media",
                "content", "seo", "campaign", "engagement",
                "营销", "品牌", "广告", "市场", "客户",
            ],
        ),
        (
            "manufacturing",
            vec![
                "manufacturing", "factory", "production", "industrial",
                "制造", "工厂", "生产", "工业", "制造业",
            ],
        ),
        (
            "retail",
            vec![
                "retail", "shop", "store", "e-commerce",
                "零售", "商店", "电商",
            ],
        ),
    ];

    let mut scores: Vec<(&str, usize)> = domain_keywords
        .iter()
        .map(|(domain, keywords)| {
            let score = keywords
                .iter()
                .filter(|kw| content_lower.contains(*kw))
                .count();
            (*domain, score)
        })
        .collect();

    scores.sort_by(|a, b| b.1.cmp(&a.1));

    if let Some((domain, score)) = scores.first()
        && *score > 0
    {
        return domain.to_string();
    }

    "unclassified".to_string()
}

/// Calculate SHA256 hash of content
pub(crate) fn calculate_content_hash(content: &[u8]) -> String {
    use hex::encode;
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(content);
    encode(hasher.finalize())
}

/// Calculate query hash for cache
pub(crate) fn calculate_query_hash(query: &str, domain: &Option<String>) -> u64 {
    use std::collections::hash_map::DefaultHasher;

    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    if let Some(d) = domain {
        d.hash(&mut hasher);
    }
    hasher.finish()
}

/// Calculate content hash (truncated)
pub(crate) fn calculate_hash(content: &str) -> String {
    use hex::encode;
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(content);
    encode(&hasher.finalize()[..16])
}

/// Simple pseudo-random number generator based on string
pub(crate) fn rand_simple(s: &str) -> usize {
    let mut hash: usize = 0;
    for (i, c) in s.bytes().enumerate() {
        hash = hash.wrapping_add((c as usize).wrapping_mul(i.wrapping_add(1)));
    }
    hash
}

/// Comprehensive claim access check
#[allow(dead_code)]
pub(crate) fn check_claim_access(claim: &EpistemicClaim, _tenant_id: &str, roles: &[String]) -> bool {
    let envelope = &claim.access_envelope;

    match envelope.visibility {
        Visibility::Public => {
            if envelope.gdpr_applicable {
                tracing::debug!(claim_id = %claim.id, "GDPR applicable claim accessed");
            }
            true
        }
        Visibility::CrossTenant => true,
        Visibility::Tenant => true,
        Visibility::Team => {
            envelope.allowed_roles.is_empty()
                || roles.iter().any(|r| envelope.allowed_roles.contains(r))
        }
        Visibility::Private => {
            !envelope.allowed_roles.is_empty()
                && roles.iter().any(|r| envelope.allowed_roles.contains(r))
        }
    }
}

/// Calculate anomaly score based on feedback history
pub(crate) fn calculate_anomaly_score(history: &[AgentFeedback], current_success: bool) -> f64 {
    if history.is_empty() {
        return if current_success { 0.0 } else { 0.5 };
    }

    let recent_feedback: Vec<_> = history.iter().rev().take(10).collect();
    let total = recent_feedback.len();
    let successes = recent_feedback.iter().filter(|f| f.success).count();
    let failure_streak = recent_feedback.iter().take_while(|f| !f.success).count();

    let base_score = if current_success { 0.0 } else { 0.3 };
    let streak_factor = (failure_streak as f64 * 0.1).min(0.3);
    let rate_factor = if total > 5 {
        let recent_rate = successes as f64 / total as f64;
        if recent_rate < 0.5 && !current_success {
            (0.5 - recent_rate) * 0.3
        } else {
            0.0
        }
    } else {
        0.0
    };

    (base_score + streak_factor + rate_factor).min(1.0)
}

/// Generate suggested sources for filling a knowledge gap
pub(crate) fn generate_gap_suggestions(domain: &str, description: &str) -> Vec<String> {
    let mut suggestions = Vec::new();

    match domain.to_lowercase().as_str() {
        "technical" | "technology" => {
            suggestions.push("Technical documentation search".to_string());
            suggestions.push("API reference lookup".to_string());
            suggestions.push("Community forums (Stack Overflow, Reddit)".to_string());
        }
        "business" | "market" => {
            suggestions.push("Industry reports".to_string());
            suggestions.push("Competitor analysis".to_string());
            suggestions.push("Market research databases".to_string());
        }
        "scientific" | "research" => {
            suggestions.push("Academic papers (arXiv, Google Scholar)".to_string());
            suggestions.push("Research institutions".to_string());
            suggestions.push("Scientific databases".to_string());
        }
        _ => {
            suggestions.push("Web search".to_string());
            suggestions.push("Domain expert consultation".to_string());
        }
    }

    let desc_lower = description.to_lowercase();
    if desc_lower.contains("how") || desc_lower.contains("tutorial") {
        suggestions.push("Step-by-step guides".to_string());
    }
    if desc_lower.contains("best") || desc_lower.contains("practice") {
        suggestions.push("Best practices documentation".to_string());
    }
    if desc_lower.contains("compare") || desc_lower.contains("vs") {
        suggestions.push("Comparison articles".to_string());
    }

    suggestions
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use cogkos_core::models::*;

    #[test]
    fn test_query_vector_deterministic() {
        let v1 = generate_query_vector("test query");
        let v2 = generate_query_vector("test query");
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_query_vector_dimensions() {
        let v = generate_query_vector("hello");
        assert_eq!(v.len(), 1536);
    }

    #[test]
    fn test_query_vector_different_queries() {
        let v1 = generate_query_vector("query A");
        let v2 = generate_query_vector("query B");
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_extract_domain_arxiv_tag() {
        assert_eq!(extract_domain("Paper about [cs.AI] models"), "cs_AI");
        assert_eq!(extract_domain("[math.CO] combinatorics"), "math_CO");
    }

    #[test]
    fn test_extract_domain_keyword_cs() {
        assert_eq!(extract_domain("A deep learning algorithm for image classification"), "cs");
    }

    #[test]
    fn test_extract_domain_keyword_math() {
        assert_eq!(extract_domain("A new theorem in combinatorics"), "math");
    }

    #[test]
    fn test_extract_domain_keyword_chinese() {
        assert_eq!(extract_domain("基于神经网络的图像分类算法"), "cs");
    }

    #[test]
    fn test_extract_domain_unknown() {
        assert_eq!(extract_domain("Some random text about cooking"), "unclassified");
    }

    #[test]
    fn test_content_hash_deterministic() {
        assert_eq!(calculate_content_hash(b"hello"), calculate_content_hash(b"hello"));
    }

    #[test]
    fn test_content_hash_different_content() {
        assert_ne!(calculate_content_hash(b"hello"), calculate_content_hash(b"world"));
    }

    #[test]
    fn test_content_hash_is_hex() {
        let h = calculate_content_hash(b"test");
        assert!(h.len() == 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_anomaly_score_empty_history_success() {
        assert_eq!(calculate_anomaly_score(&[], true), 0.0);
    }

    #[test]
    fn test_anomaly_score_empty_history_failure() {
        assert_eq!(calculate_anomaly_score(&[], false), 0.5);
    }

    #[test]
    fn test_anomaly_score_bounded() {
        let feedbacks: Vec<AgentFeedback> = (0..20)
            .map(|_| AgentFeedback {
                query_hash: 0,
                agent_id: "test".to_string(),
                success: false,
                feedback_note: None,
                timestamp: chrono::Utc::now(),
            })
            .collect();
        let score = calculate_anomaly_score(&feedbacks, false);
        assert!(score <= 1.0);
        assert!(score > 0.0);
    }

    #[test]
    fn test_gap_suggestions_technical() {
        let s = generate_gap_suggestions("technical", "API documentation");
        assert!(!s.is_empty());
        assert!(s.iter().any(|s| s.contains("Technical")));
    }

    #[test]
    fn test_gap_suggestions_with_how() {
        let s = generate_gap_suggestions("general", "how to deploy");
        assert!(s.iter().any(|s| s.contains("Step-by-step")));
    }

    #[test]
    fn test_gap_suggestions_with_compare() {
        let s = generate_gap_suggestions("general", "compare PostgreSQL vs MySQL");
        assert!(s.iter().any(|s| s.contains("Comparison")));
    }

    #[test]
    fn test_query_hash_deterministic() {
        assert_eq!(calculate_query_hash("test", &None), calculate_query_hash("test", &None));
    }

    #[test]
    fn test_query_hash_domain_matters() {
        assert_ne!(calculate_query_hash("test", &None), calculate_query_hash("test", &Some("cs".to_string())));
    }

    #[test]
    fn test_claim_access_public() {
        let mut claim = EpistemicClaim::new(
            "test content".to_string(), "tenant-1".to_string(), NodeType::Entity,
            Claimant::System, AccessEnvelope::new("tenant-1"),
            ProvenanceRecord {
                source_id: "test".to_string(), source_type: "test".to_string(),
                ingestion_method: "test".to_string(), original_url: None, audit_hash: "test".to_string(),
            },
        );
        claim.access_envelope.visibility = Visibility::Public;
        assert!(check_claim_access(&claim, "tenant-1", &[]));
    }

    #[test]
    fn test_claim_access_private_with_role() {
        let mut claim = EpistemicClaim::new(
            "test content".to_string(), "tenant-1".to_string(), NodeType::Entity,
            Claimant::System, AccessEnvelope::new("tenant-1"),
            ProvenanceRecord {
                source_id: "test".to_string(), source_type: "test".to_string(),
                ingestion_method: "test".to_string(), original_url: None, audit_hash: "test".to_string(),
            },
        );
        claim.access_envelope.visibility = Visibility::Private;
        claim.access_envelope.allowed_roles = vec!["admin".to_string()];
        assert!(check_claim_access(&claim, "tenant-1", &["admin".to_string()]));
        assert!(!check_claim_access(&claim, "tenant-1", &["reader".to_string()]));
        assert!(!check_claim_access(&claim, "tenant-1", &[]));
    }
}
