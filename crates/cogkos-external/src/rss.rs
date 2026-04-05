//! RSS Feed Connector for external knowledge subscription
//!
//! Provides RSS/Atom feed parsing and auto-ingestion into the knowledge system.

use crate::error::{ExternalError, Result};
use crate::types::{ExternalDocument, SourceType};
use chrono::{DateTime, Utc};
use feed_rs::parser;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// RSS Feed configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssFeedConfig {
    /// Feed URL
    pub url: String,
    /// Polling interval in seconds
    pub poll_interval_secs: u64,
    /// Maximum items to fetch per poll
    pub max_items: usize,
    /// Whether to fetch full content
    pub fetch_full_content: bool,
    /// Custom headers for the request
    pub headers: HashMap<String, String>,
}

impl Default for RssFeedConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            poll_interval_secs: 3600, // 1 hour
            max_items: 20,
            fetch_full_content: false,
            headers: HashMap::new(),
        }
    }
}

/// RSS Feed entry/item
#[derive(Debug, Clone)]
pub struct RssFeedItem {
    pub id: String,
    pub title: String,
    pub content: String,
    pub link: String,
    pub published: Option<DateTime<Utc>>,
    pub author: Option<String>,
    pub categories: Vec<String>,
}

/// RSS Connector for fetching and parsing feeds
pub struct RssConnector {
    client: Client,
    config: RssFeedConfig,
}

impl RssConnector {
    /// Create a new RSS connector
    pub fn new(config: RssFeedConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ExternalError::NetworkError(format!("HTTP client: {}", e)))?;

        Ok(Self { client, config })
    }

    /// Fetch and parse an RSS feed
    pub async fn fetch_feed(&self) -> Result<Vec<RssFeedItem>> {
        let mut request = self.client.get(&self.config.url);

        // Add custom headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(ExternalError::ApiError {
                provider: "rss".to_string(),
                message: format!("HTTP error: {}", response.status()),
            });
        }

        let bytes = response.bytes().await?;
        let feed = parser::parse(&bytes[..])?;

        let items: Vec<RssFeedItem> = feed
            .entries
            .iter()
            .take(self.config.max_items)
            .map(|entry| {
                // entry.id is a String, use it directly or fall back to link
                let id = if !entry.id.is_empty() {
                    entry.id.clone()
                } else {
                    entry
                        .links
                        .first()
                        .map(|l| l.href.clone())
                        .unwrap_or_default()
                };

                let content = entry
                    .content
                    .as_ref()
                    .and_then(|c| c.body.clone())
                    .or_else(|| entry.summary.as_ref().map(|s| s.content.clone()))
                    .unwrap_or_default();

                let published = entry.published.or(entry.updated);

                let author = entry.authors.first().map(|a| a.name.clone());

                let categories = entry.categories.iter().map(|c| c.term.clone()).collect();

                RssFeedItem {
                    id,
                    title: entry
                        .title
                        .as_ref()
                        .map(|t| t.content.clone())
                        .unwrap_or_default(),
                    content,
                    link: entry
                        .links
                        .first()
                        .map(|l| l.href.clone())
                        .unwrap_or_default(),
                    published,
                    author,
                    categories,
                }
            })
            .collect();

        Ok(items)
    }

    /// Convert RSS feed items to ExternalDocument
    pub fn items_to_documents(
        &self,
        items: Vec<RssFeedItem>,
        feed_title: &str,
    ) -> Vec<ExternalDocument> {
        items
            .into_iter()
            .map(|item| ExternalDocument {
                id: format!("rss:{}:{}", feed_title, item.id),
                title: item.title.clone(),
                content: item.content,
                url: item.link,
                source: feed_title.to_string(),
                source_type: SourceType::RssFeed,
                published_at: item.published,
                authors: item.author.map(|a| vec![a]).unwrap_or_default(),
                tags: item.categories,
                metadata: serde_json::json!({
                    "feed_url": self.config.url,
                }),
                confidence: 0.9,
                fetched_at: Utc::now(),
            })
            .collect()
    }
}

/// Manager for multiple RSS feeds
pub struct RssFeedManager {
    feeds: Arc<RwLock<HashMap<String, (RssConnector, RssFeedConfig)>>>,
}

impl RssFeedManager {
    /// Create a new RSS feed manager
    pub fn new() -> Self {
        Self {
            feeds: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a feed to the manager
    pub async fn add_feed(&self, name: String, config: RssFeedConfig) -> Result<()> {
        let connector = RssConnector::new(config.clone())?;
        let mut feeds = self.feeds.write().await;
        feeds.insert(name.clone(), (connector, config));
        Ok(())
    }

    /// Remove a feed from the manager
    pub async fn remove_feed(&self, name: &str) {
        let mut feeds = self.feeds.write().await;
        feeds.remove(name);
    }

    /// Fetch all feeds and return documents
    pub async fn fetch_all(&self) -> HashMap<String, Vec<ExternalDocument>> {
        let feeds = self.feeds.read().await;
        let mut results = HashMap::new();

        // Fetch all feeds concurrently
        let futures: Vec<_> = feeds
            .iter()
            .filter_map(|(name, (connector, _))| {
                let name = name.clone();
                match RssConnector::new(connector.config.clone()) {
                    Ok(connector) => Some(async move {
                        let items = connector.fetch_feed().await;
                        (name, items)
                    }),
                    Err(e) => {
                        tracing::warn!("Failed to create RSS connector for {}: {}", name, e);
                        None
                    }
                }
            })
            .collect();

        let fetched = stream::iter(futures)
            .buffer_unordered(5)
            .collect::<Vec<_>>()
            .await;

        for (name, items_result) in fetched {
            match items_result {
                Ok(items) => {
                    let docs = match RssConnector::new(RssFeedConfig::default()) {
                        Ok(c) => c.items_to_documents(items, &name),
                        Err(e) => {
                            tracing::warn!("Failed to create RSS connector for documents: {}", e);
                            continue;
                        }
                    };
                    if !docs.is_empty() {
                        results.insert(name, docs);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch feed {}: {}", name, e);
                }
            }
        }

        results
    }

    /// Get list of managed feeds
    pub async fn list_feeds(&self) -> Vec<String> {
        let feeds = self.feeds.read().await;
        feeds.keys().cloned().collect()
    }
}

impl Default for RssFeedManager {
    fn default() -> Self {
        Self::new()
    }
}
