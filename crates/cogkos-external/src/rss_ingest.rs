//! RSS Ingestion Service
//!
//! Integrates RSS/Atom feed polling with the ingestion pipeline.
//! Provides automatic feed polling and document ingestion to the knowledge base.

use crate::{
    Result,
    error::ExternalError,
    rss::{RssFeedEntry, RssSubscriptionConfig, RssSubscriptionManager},
    types::ExternalDocument,
};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};
use tracing::{debug, error, info, warn};

/// RSS document ready for ingestion
#[derive(Debug, Clone)]
pub struct RssIngestDocument {
    pub id: String,
    pub title: String,
    pub content: String,
    pub url: String,
    pub feed_url: String,
    pub feed_name: String,
    pub published_at: Option<chrono::DateTime<Utc>>,
    pub author: Option<String>,
    pub categories: Vec<String>,
}

impl From<RssFeedEntry> for RssIngestDocument {
    fn from(entry: RssFeedEntry) -> Self {
        Self {
            id: entry.id,
            title: entry.title,
            content: entry.content,
            url: entry.url,
            feed_url: String::new(),
            feed_name: String::new(),
            published_at: entry.published_at,
            author: entry.author,
            categories: entry.categories,
        }
    }
}

/// RSS ingestion service configuration
#[derive(Debug, Clone)]
pub struct RssIngestConfig {
    /// Default polling interval in seconds
    pub default_poll_interval: u64,
    /// Maximum entries to process per feed per poll
    pub max_entries_per_poll: usize,
    /// Enable deduplication
    pub enable_deduplication: bool,
    /// Rate limit: requests per minute
    pub rate_limit_per_minute: u32,
    /// Batch size for ingestion
    pub ingestion_batch_size: usize,
}

impl Default for RssIngestConfig {
    fn default() -> Self {
        Self {
            default_poll_interval: 300, // 5 minutes
            max_entries_per_poll: 50,
            enable_deduplication: true,
            rate_limit_per_minute: 30,
            ingestion_batch_size: 10,
        }
    }
}

/// RSS ingestion service
pub struct RssIngestService {
    manager: RssSubscriptionManager,
    config: RssIngestConfig,
    feed_names: Arc<tokio::sync::RwLock<HashMap<String, String>>>, // feed_id -> name
}

impl RssIngestService {
    /// Create new RSS ingestion service
    pub fn new(config: RssIngestConfig) -> Self {
        Self {
            manager: RssSubscriptionManager::new(),
            config,
            feed_names: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Create with custom subscription manager
    pub fn with_manager(manager: RssSubscriptionManager, config: RssIngestConfig) -> Self {
        Self {
            manager,
            config,
            feed_names: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe to a feed with ingestion enabled
    pub async fn subscribe(
        &self,
        url: impl Into<String>,
        name: impl Into<String>,
        poll_interval: Option<u64>,
    ) -> Result<String> {
        let url = url.into();
        let name = name.into();

        let config = RssSubscriptionConfig {
            url: url.clone(),
            poll_interval_seconds: poll_interval.unwrap_or(self.config.default_poll_interval),
            max_entries_per_poll: self.config.max_entries_per_poll,
            rate_limit_per_minute: self.config.rate_limit_per_minute,
            timeout_seconds: 30,
        };

        let feed_id = self.manager.add_feed(config).await?;
        self.feed_names.write().await.insert(feed_id.clone(), name);

        info!("Subscribed to RSS feed: {} (id: {})", url, feed_id);
        Ok(feed_id)
    }

    /// Unsubscribe from a feed
    pub async fn unsubscribe(&self, feed_id: &str) -> Result<()> {
        self.manager.remove_feed(feed_id).await?;
        self.feed_names.write().await.remove(feed_id);
        info!("Unsubscribed from RSS feed: {}", feed_id);
        Ok(())
    }

    /// Poll all feeds and return new documents
    pub async fn poll_all(&self) -> Vec<RssIngestDocument> {
        let results = self.manager.poll_all().await;
        let feed_names = self.feed_names.read().await;

        let mut documents = Vec::new();

        for (feed_id, entries) in results {
            let feed_name = feed_names
                .get(&feed_id)
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string());

            // Get feed URL from manager
            let feeds = self.manager.list_feeds().await;
            let feed_url = feeds
                .iter()
                .find(|(id, _)| id == &feed_id)
                .map(|(_, config)| config.url.clone())
                .unwrap_or_default();

            for entry in entries {
                let mut doc: RssIngestDocument = entry.into();
                doc.feed_name = feed_name.clone();
                doc.feed_url = feed_url.clone();
                documents.push(doc);
            }
        }

        documents
    }

    /// Poll a specific feed
    pub async fn poll_feed(&self, feed_id: &str) -> Result<Vec<RssIngestDocument>> {
        let feeds = self.manager.list_feeds().await;
        let (_, config) = feeds
            .into_iter()
            .find(|(id, _)| id == feed_id)
            .ok_or_else(|| ExternalError::InvalidParams(format!("Feed {} not found", feed_id)))?;

        let entries = self.manager.poll_feed(feed_id, &config).await?;
        let feed_names = self.feed_names.read().await;
        let feed_name = feed_names
            .get(feed_id)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());

        let mut documents = Vec::new();
        for entry in entries {
            let mut doc: RssIngestDocument = entry.into();
            doc.feed_name = feed_name.clone();
            doc.feed_url = config.url.clone();
            documents.push(doc);
        }

        Ok(documents)
    }

    /// Start automatic polling and ingestion
    /// Returns a receiver channel for ingested documents
    pub fn start_auto_ingest(self: Arc<Self>) -> mpsc::Receiver<Vec<RssIngestDocument>> {
        let (tx, rx) = mpsc::channel(100);
        let self_clone = Arc::clone(&self);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(60)); // Check every minute

            loop {
                ticker.tick().await;

                // Get list of feeds and their poll intervals
                let feeds = self_clone.manager.list_feeds().await;

                for (feed_id, _config) in feeds {
                    // Simple rate limiting: check if enough time has passed
                    // In production, track last poll time per feed
                    debug!("Checking feed {} for new entries", feed_id);

                    match self_clone.poll_feed(&feed_id).await {
                        Ok(docs) if !docs.is_empty() => {
                            info!("Found {} new documents from feed {}", docs.len(), feed_id);
                            if tx.send(docs).await.is_err() {
                                error!("Ingest channel closed, stopping auto-ingest");
                                return;
                            }
                        }
                        Ok(_) => {
                            debug!("No new documents from feed {}", feed_id);
                        }
                        Err(e) => {
                            warn!("Failed to poll feed {}: {}", feed_id, e);
                        }
                    }

                    // Small delay between feeds to avoid rate limiting
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        rx
    }

    /// Convert RSS ingest document to ExternalDocument
    pub fn to_external_document(doc: &RssIngestDocument) -> ExternalDocument {
        ExternalDocument {
            id: format!("rss:{}", doc.id),
            title: doc.title.clone(),
            content: doc.content.clone(),
            url: doc.url.clone(),
            source: format!("RSS: {}", doc.feed_name),
            source_type: crate::types::SourceType::RssFeed,
            published_at: doc.published_at,
            authors: doc.author.clone().into_iter().collect(),
            tags: doc.categories.clone(),
            metadata: serde_json::json!({
                "feed_url": doc.feed_url,
                "feed_name": doc.feed_name,
                "original_id": doc.id,
            }),
            confidence: 0.9,
            fetched_at: Utc::now(),
        }
    }

    /// List all subscribed feeds
    pub async fn list_feeds(&self) -> Vec<(String, String, RssSubscriptionConfig)> {
        let feeds = self.manager.list_feeds().await;
        let names = self.feed_names.read().await;

        feeds
            .into_iter()
            .map(|(id, config)| {
                let name = names.get(&id).cloned().unwrap_or_default();
                (id, name, config)
            })
            .collect()
    }

    /// Get subscription manager reference
    pub fn manager(&self) -> &RssSubscriptionManager {
        &self.manager
    }
}

impl Default for RssIngestService {
    fn default() -> Self {
        Self::new(RssIngestConfig::default())
    }
}

/// Builder for RSS ingestion service
pub struct RssIngestServiceBuilder {
    config: RssIngestConfig,
    feeds: Vec<(String, String, Option<u64>)>, // (url, name, poll_interval)
}

impl RssIngestServiceBuilder {
    /// Create new builder
    pub fn new() -> Self {
        Self {
            config: RssIngestConfig::default(),
            feeds: Vec::new(),
        }
    }

    /// Set default poll interval
    pub fn poll_interval(mut self, seconds: u64) -> Self {
        self.config.default_poll_interval = seconds;
        self
    }

    /// Set max entries per poll
    pub fn max_entries(mut self, count: usize) -> Self {
        self.config.max_entries_per_poll = count;
        self
    }

    /// Set rate limit
    pub fn rate_limit(mut self, per_minute: u32) -> Self {
        self.config.rate_limit_per_minute = per_minute;
        self
    }

    /// Add a feed subscription
    pub fn add_feed(
        mut self,
        url: impl Into<String>,
        name: impl Into<String>,
        poll_interval: Option<u64>,
    ) -> Self {
        self.feeds.push((url.into(), name.into(), poll_interval));
        self
    }

    /// Build the service
    pub async fn build(self) -> Result<RssIngestService> {
        let service = RssIngestService::new(self.config);

        for (url, name, interval) in self.feeds {
            service.subscribe(url, name, interval).await?;
        }

        Ok(service)
    }
}

impl Default for RssIngestServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rss_ingest_document_conversion() {
        let doc = RssIngestDocument {
            id: "test-1".to_string(),
            title: "Test Article".to_string(),
            content: "Test content here".to_string(),
            url: "https://example.com/article".to_string(),
            feed_url: "https://example.com/feed.xml".to_string(),
            feed_name: "Test Feed".to_string(),
            published_at: Some(Utc::now()),
            author: Some("Test Author".to_string()),
            categories: vec!["tech".to_string(), "rust".to_string()],
        };

        let external = RssIngestService::to_external_document(&doc);

        assert_eq!(external.title, "Test Article");
        assert_eq!(external.source_type, crate::types::SourceType::RssFeed);
        assert_eq!(external.confidence, 0.9);
        assert_eq!(external.authors, vec!["Test Author"]);
        assert_eq!(external.tags, vec!["tech", "rust"]);
    }

    #[test]
    fn test_ingest_config_default() {
        let config = RssIngestConfig::default();
        assert_eq!(config.default_poll_interval, 300);
        assert_eq!(config.max_entries_per_poll, 50);
        assert!(config.enable_deduplication);
        assert_eq!(config.rate_limit_per_minute, 30);
    }

    #[test]
    fn test_builder_pattern() {
        let builder = RssIngestServiceBuilder::new()
            .poll_interval(600)
            .max_entries(100)
            .rate_limit(60)
            .add_feed("https://example.com/feed.xml", "Example", Some(300));

        assert_eq!(builder.config.default_poll_interval, 600);
        assert_eq!(builder.config.max_entries_per_poll, 100);
        assert_eq!(builder.config.rate_limit_per_minute, 60);
        assert_eq!(builder.feeds.len(), 1);
    }
}
