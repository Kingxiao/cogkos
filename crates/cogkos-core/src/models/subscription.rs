use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

use super::Claimant;

/// External subscription source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSource {
    pub id: Uuid,
    pub name: String,
    pub source_type: SubscriptionType,
    pub config: serde_json::Value,
    /// Poll interval in seconds
    pub poll_interval_secs: u64,
    pub claimant_template: Claimant,
    pub base_confidence: f64,
    #[serde(default)]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_polled: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub error_count: u32,
    pub tenant_id: String,
}

impl SubscriptionSource {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs)
    }
}

/// Subscription type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionType {
    Rss,
    Webhook,
    Api,
}

impl SubscriptionType {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            SubscriptionType::Rss => "rss",
            SubscriptionType::Webhook => "webhook",
            SubscriptionType::Api => "api",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "rss" => SubscriptionType::Rss,
            "webhook" => SubscriptionType::Webhook,
            "api" => SubscriptionType::Api,
            _ => SubscriptionType::Rss,
        }
    }
}

/// Meta knowledge entry for federation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaKnowledgeEntry {
    pub instance_id: String,
    #[serde(default)]
    pub domain_tags: Vec<String>,
    pub expertise_score: f64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

/// Federation health check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationHealthCheck {
    pub diversity_entropy: f64,
    pub independence_score: f64,
    pub centralization_gini: f64,
    pub aggregation_vs_best: f64,
}
