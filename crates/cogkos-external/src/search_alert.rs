//! Search Alert Subscription Module
//!
//! Implements keyword-based search monitoring similar to Google Alerts:
//! - Configurable keywords and polling intervals
//! - Integration with search engines
//! - New result detection and alerting
//! - Output to L9 ingestion pipeline

use crate::error::{ExternalError, Result};
use crate::search::SearchEngine;
use crate::types::{ExternalDocument, SearchQuery, SourceType};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Configuration for search alert subscription
#[derive(Debug, Clone)]
pub struct SearchAlertConfig {
    /// Keywords to monitor
    pub keywords: Vec<String>,
    /// Search engine to use
    pub engine_name: String,
    /// Poll interval in seconds
    pub poll_interval_seconds: u64,
    /// Maximum results per search
    pub max_results: usize,
    /// Language filter
    pub language: Option<String>,
}

impl Default for SearchAlertConfig {
    fn default() -> Self {
        Self {
            keywords: Vec::new(),
            engine_name: "duckduckgo".to_string(),
            poll_interval_seconds: 3600, // 1 hour
            max_results: 10,
            language: None,
        }
    }
}

/// Search alert entry with detected new results
#[derive(Debug, Clone)]
pub struct SearchAlertEntry {
    pub keyword: String,
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub detected_at: DateTime<Utc>,
}

/// Search alert subscription manager
#[derive(Clone)]
pub struct SearchAlertSubscriptionManager {
    client: reqwest::Client,
    alerts: Arc<RwLock<HashMap<String, SearchAlertConfig>>>,
    seen_results: Arc<RwLock<HashMap<String, HashSet<String>>>>, // alert_id -> set of result URLs
    search_engine: Arc<dyn SearchEngine>,
}

impl SearchAlertSubscriptionManager {
    /// Create new search alert subscription manager with a search engine
    pub fn new(search_engine: Arc<dyn SearchEngine>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ExternalError::NetworkError(format!("HTTP client: {}", e)))?;

        Ok(Self {
            client,
            alerts: Arc::new(RwLock::new(HashMap::new())),
            seen_results: Arc::new(RwLock::new(HashMap::new())),
            search_engine,
        })
    }

    /// Add a search alert subscription
    pub async fn add_alert(&self, config: SearchAlertConfig) -> Result<String> {
        if config.keywords.is_empty() {
            return Err(ExternalError::InvalidParams(
                "At least one keyword is required".to_string(),
            ));
        }

        let alert_id = uuid::Uuid::new_v4().to_string();
        self.alerts.write().await.insert(alert_id.clone(), config);
        self.seen_results
            .write()
            .await
            .insert(alert_id.clone(), HashSet::new());

        Ok(alert_id)
    }

    /// Remove a search alert subscription
    pub async fn remove_alert(&self, alert_id: &str) -> Result<()> {
        self.alerts.write().await.remove(alert_id);
        self.seen_results.write().await.remove(alert_id);
        Ok(())
    }

    /// List all configured alerts
    pub async fn list_alerts(&self) -> Vec<(String, SearchAlertConfig)> {
        let alerts = self.alerts.read().await;
        alerts.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Check for new results for a specific alert
    pub async fn check_alert(&self, alert_id: &str) -> Result<Vec<SearchAlertEntry>> {
        let config = {
            let alerts = self.alerts.read().await;
            alerts.get(alert_id).cloned()
        };

        let config = config.ok_or_else(|| {
            ExternalError::InvalidParams(format!("Alert {} not found", alert_id))
        })?;

        let mut new_entries = Vec::new();

        for keyword in &config.keywords {
            let query = SearchQuery {
                query: keyword.clone(),
                filters: vec![],
                limit: config.max_results,
                offset: 0,
                sort_by: crate::types::SortOption::DateDesc,
            };

            let result = self.search_engine.search(&query).await;

            match result {
                Ok(search_result) => {
                    let mut seen = self.seen_results.write().await;
                    let seen_set = seen.entry(alert_id.to_string()).or_insert_with(HashSet::new);

                    for doc in search_result.documents {
                        if !seen_set.contains(&doc.url) {
                            seen_set.insert(doc.url.clone());
                            new_entries.push(SearchAlertEntry {
                                keyword: keyword.clone(),
                                url: doc.url,
                                title: doc.title,
                                snippet: doc.content.chars().take(200).collect(),
                                detected_at: Utc::now(),
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Search failed for keyword {}: {}", keyword, e);
                }
            }
        }

        Ok(new_entries)
    }

    /// Check all alerts and return new results
    pub async fn check_all_alerts(&self) -> HashMap<String, Vec<SearchAlertEntry>> {
        let alerts = self.alerts.read().await.clone();
        let mut results = HashMap::new();

        for (alert_id, _) in alerts {
            match self.check_alert(&alert_id).await {
                Ok(entries) => {
                    if !entries.is_empty() {
                        results.insert(alert_id, entries);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to check alert: {}", e);
                }
            }
        }

        results
    }

    /// Convert search alert entries to ExternalDocuments for L9 ingestion
    pub fn to_external_documents(&self, entries: Vec<SearchAlertEntry>) -> Vec<ExternalDocument> {
        entries
            .into_iter()
            .map(|entry| {
                let keyword = entry.keyword.clone();
                ExternalDocument {
                    id: format!("search_alert:{}:{}", keyword, uuid::Uuid::new_v4()),
                    title: entry.title,
                    content: entry.snippet,
                    url: entry.url,
                    source: format!("search_alert:{}", keyword),
                    source_type: SourceType::SearchEngine,
                    published_at: Some(entry.detected_at),
                    authors: vec![],
                    tags: vec![keyword.clone()],
                    metadata: serde_json::json!({ "keyword": keyword }),
                    confidence: 0.7,
                    fetched_at: entry.detected_at,
                }
            })
            .collect()
    }
}

impl Default for SearchAlertSubscriptionManager {
    fn default() -> Self {
        let engine = crate::search::DuckDuckGoConnector::new()
            .expect("valid default HTTP client config");
        Self::new(Arc::new(engine))
            .expect("valid default HTTP client config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_alert_creation() {
        let manager = SearchAlertSubscriptionManager::default();

        let config = SearchAlertConfig {
            keywords: vec!["rust programming".to_string()],
            engine_name: "duckduckgo".to_string(),
            poll_interval_seconds: 3600,
            max_results: 10,
            language: None,
        };

        let alert_id = manager.add_alert(config).await.unwrap();
        assert!(!alert_id.is_empty());

        let alerts = manager.list_alerts().await;
        assert_eq!(alerts.len(), 1);
    }

    #[tokio::test]
    async fn test_search_alert_removal() {
        let manager = SearchAlertSubscriptionManager::default();

        let config = SearchAlertConfig {
            keywords: vec!["test".to_string()],
            ..Default::default()
        };

        let alert_id = manager.add_alert(config).await.unwrap();
        manager.remove_alert(&alert_id).await.unwrap();

        let alerts = manager.list_alerts().await;
        assert!(alerts.is_empty());
    }
}
