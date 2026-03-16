pub mod arxiv;
pub mod error;
pub mod polling;
pub mod rss;
pub mod search;
pub mod types;
pub mod webhook;
pub mod wikipedia;

pub use arxiv::ArxivConnector;
pub use error::{ExternalError, Result};
pub use polling::RssSubscriptionManager;
pub use rss::{
    RssConnector, RssFeedConfig, RssFeedItem,
    RssFeedManager,
};
pub use search::{AggregatedSearchEngine, DuckDuckGoConnector, SearchEngine, SerpApiConnector};
pub use types::{ConnectorConfig, ExternalDocument, SearchQuery, SearchResult, SourceType};
pub use webhook::{
    WebhookEventHandler, WebhookEventType, WebhookManager, WebhookPayload, WebhookReceiver,
    WebhookServer, WebhookSignatureValidator, WebhookSubscription,
};
pub use wikipedia::WikipediaConnector;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// External knowledge manager that coordinates multiple sources
pub struct ExternalKnowledgeManager {
    wikipedia: Option<Arc<WikipediaConnector>>,
    arxiv: Option<Arc<ArxivConnector>>,
    search_engines: Vec<Arc<dyn SearchEngine>>,
    rss_polling: Arc<RssSubscriptionManager>,
    webhook_manager: Arc<WebhookManager>,
}

impl ExternalKnowledgeManager {
    pub fn new() -> Self {
        Self {
            wikipedia: None,
            arxiv: None,
            search_engines: Vec::new(),
            rss_polling: Arc::new(RssSubscriptionManager::new()),
            webhook_manager: Arc::new(WebhookManager::new()),
        }
    }

    pub fn rss_polling(&self) -> Arc<RssSubscriptionManager> {
        Arc::clone(&self.rss_polling)
    }

    pub fn webhook_manager(&self) -> Arc<WebhookManager> {
        Arc::clone(&self.webhook_manager)
    }

    pub fn with_wikipedia(mut self) -> Result<Self> {
        self.wikipedia = Some(Arc::new(WikipediaConnector::new()?));
        Ok(self)
    }

    pub fn with_wikipedia_config(mut self, config: ConnectorConfig) -> Result<Self> {
        self.wikipedia = Some(Arc::new(WikipediaConnector::with_config(config)?));
        Ok(self)
    }

    pub fn with_arxiv(mut self) -> Result<Self> {
        self.arxiv = Some(Arc::new(ArxivConnector::new()?));
        Ok(self)
    }

    pub fn with_arxiv_config(mut self, config: ConnectorConfig) -> Result<Self> {
        self.arxiv = Some(Arc::new(ArxivConnector::with_config(config)?));
        Ok(self)
    }

    pub fn with_duckduckgo(mut self) -> Result<Self> {
        self.search_engines
            .push(Arc::new(DuckDuckGoConnector::new()?));
        Ok(self)
    }

    pub fn with_serpapi(mut self, api_key: impl Into<String>) -> Result<Self> {
        self.search_engines
            .push(Arc::new(SerpApiConnector::new(api_key)?));
        Ok(self)
    }

    /// Search Wikipedia
    pub async fn search_wikipedia(&self, query: &SearchQuery) -> Result<SearchResult> {
        match &self.wikipedia {
            Some(wiki) => wiki.search_documents(query).await,
            None => Err(ExternalError::InvalidParams(
                "Wikipedia connector not configured".to_string(),
            )),
        }
    }

    /// Get a specific Wikipedia article
    pub async fn get_wikipedia_article(&self, title: &str) -> Result<ExternalDocument> {
        match &self.wikipedia {
            Some(wiki) => wiki.get_page(title).await,
            None => Err(ExternalError::InvalidParams(
                "Wikipedia connector not configured".to_string(),
            )),
        }
    }

    /// Search arXiv
    pub async fn search_arxiv(&self, query: &SearchQuery) -> Result<SearchResult> {
        match &self.arxiv {
            Some(arxiv) => arxiv.search_documents(query).await,
            None => Err(ExternalError::InvalidParams(
                "arXiv connector not configured".to_string(),
            )),
        }
    }

    /// Search all configured sources
    pub async fn search_all(&self, query: &SearchQuery) -> Vec<(String, Result<SearchResult>)> {
        use futures::future::join_all;

        #[allow(clippy::type_complexity)]
        let mut futures: Vec<Pin<Box<dyn Future<Output = (String, Result<SearchResult>)> + Send>>> =
            Vec::new();

        // Wikipedia
        if let Some(wiki) = &self.wikipedia {
            let wiki_clone = Arc::clone(wiki);
            let query_clone = query.clone();
            futures.push(Box::pin(async move {
                (
                    "Wikipedia".to_string(),
                    wiki_clone.search_documents(&query_clone).await,
                )
            }));
        }

        // arXiv
        if let Some(arxiv) = &self.arxiv {
            let arxiv_clone = Arc::clone(arxiv);
            let query_clone = query.clone();
            futures.push(Box::pin(async move {
                (
                    "arXiv".to_string(),
                    arxiv_clone.search_documents(&query_clone).await,
                )
            }));
        }

        // Search engines
        for engine in &self.search_engines {
            let engine_clone = Arc::clone(engine);
            let query_clone = query.clone();
            let name = engine.name().to_string();
            futures.push(Box::pin(async move {
                (name, engine_clone.search(&query_clone).await)
            }));
        }

        join_all(futures).await
    }

    /// Search and merge results from all sources
    pub async fn search_merged(&self, query: &SearchQuery) -> Result<SearchResult> {
        let start = std::time::Instant::now();
        let results = self.search_all(query).await;

        let mut seen_urls = std::collections::HashSet::new();
        let mut merged_docs = Vec::new();

        for (source, result) in results {
            match result {
                Ok(search_result) => {
                    for doc in search_result.documents {
                        if seen_urls.insert(doc.url.clone()) {
                            merged_docs.push(doc);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Source {} failed: {}", source, e);
                }
            }
        }

        // Sort by confidence
        merged_docs.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

        // Apply limit
        merged_docs.truncate(query.limit);

        let search_time = start.elapsed().as_millis() as u64;

        Ok(SearchResult {
            total_count: merged_docs.len(),
            documents: merged_docs,
            query: query.clone(),
            search_time_ms: search_time,
        })
    }
}

impl Default for ExternalKnowledgeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for ExternalKnowledgeManager
pub struct ExternalKnowledgeManagerBuilder {
    wikipedia: Option<WikipediaConnector>,
    arxiv: Option<ArxivConnector>,
    search_engines: Vec<Box<dyn SearchEngine>>,
}

impl ExternalKnowledgeManagerBuilder {
    pub fn new() -> Self {
        Self {
            wikipedia: None,
            arxiv: None,
            search_engines: Vec::new(),
        }
    }

    pub fn wikipedia(mut self) -> Result<Self> {
        self.wikipedia = Some(WikipediaConnector::new()?);
        Ok(self)
    }

    pub fn wikipedia_with_config(mut self, config: ConnectorConfig) -> Result<Self> {
        self.wikipedia = Some(WikipediaConnector::with_config(config)?);
        Ok(self)
    }

    pub fn arxiv(mut self) -> Result<Self> {
        self.arxiv = Some(ArxivConnector::new()?);
        Ok(self)
    }

    pub fn arxiv_with_config(mut self, config: ConnectorConfig) -> Result<Self> {
        self.arxiv = Some(ArxivConnector::with_config(config)?);
        Ok(self)
    }

    pub fn duckduckgo(mut self) -> Result<Self> {
        self.search_engines
            .push(Box::new(DuckDuckGoConnector::new()?));
        Ok(self)
    }

    pub fn serpapi(mut self, api_key: impl Into<String>) -> Result<Self> {
        self.search_engines
            .push(Box::new(SerpApiConnector::new(api_key)?));
        Ok(self)
    }

    pub fn build(self) -> ExternalKnowledgeManager {
        let mut manager = ExternalKnowledgeManager::new();

        if let Some(wiki) = self.wikipedia {
            manager.wikipedia = Some(Arc::new(wiki));
        }

        if let Some(arxiv) = self.arxiv {
            manager.arxiv = Some(Arc::new(arxiv));
        }

        for engine in self.search_engines {
            manager.search_engines.push(Arc::from(engine));
        }

        manager
    }
}

impl Default for ExternalKnowledgeManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_external_document_creation() {
        let doc = ExternalDocument {
            id: "test:1".to_string(),
            title: "Test Document".to_string(),
            content: "Test content".to_string(),
            url: "https://example.com".to_string(),
            source: "Test".to_string(),
            source_type: SourceType::WebPage,
            published_at: None,
            authors: vec!["Author".to_string()],
            tags: vec!["test".to_string()],
            metadata: serde_json::json!({}),
            confidence: 0.8,
            fetched_at: Utc::now(),
        };

        assert_eq!(doc.title, "Test Document");
        assert_eq!(doc.confidence, 0.8);
    }

    #[test]
    fn test_search_query_builder() {
        let query = SearchQuery {
            query: "test".to_string(),
            filters: vec![],
            limit: 10,
            offset: 0,
            sort_by: crate::types::SortOption::Relevance,
        };

        assert_eq!(query.query, "test");
        assert_eq!(query.limit, 10);
    }
}
// TODO #203: Remove search.rs and search_alert.rs modules
// TODO #204: Remove wikipedia.rs and arxiv.rs modules
