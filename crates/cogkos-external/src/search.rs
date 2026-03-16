use crate::error::{ExternalError, Result};
use crate::types::{ConnectorConfig, ExternalDocument, SearchQuery, SearchResult, SourceType};
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use tracing::warn;

/// Generic search engine connector trait
#[async_trait]
pub trait SearchEngine: Send + Sync {
    async fn search(&self, query: &SearchQuery) -> Result<SearchResult>;
    fn name(&self) -> &'static str;
}

/// DuckDuckGo Search Connector (using HTML scraping)
/// Note: Uses DuckDuckGo HTML interface for demonstration
pub struct DuckDuckGoConnector {
    client: reqwest::Client,
    _config: ConnectorConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct DuckDuckGoResult {
    title: String,
    url: String,
    snippet: String,
}

impl DuckDuckGoConnector {
    pub fn new() -> Result<Self> {
        Self::with_config(ConnectorConfig::default())
    }

    pub fn with_config(config: ConnectorConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .map_err(|e| ExternalError::NetworkError(format!("HTTP client: {}", e)))?;

        Ok(Self {
            client,
            _config: config,
        })
    }

    async fn fetch_html(&self, query: &str) -> Result<String> {
        let url = format!("https://html.duckduckgo.com/html/?q={}", urlencode(query));

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(ExternalError::ApiError {
                provider: "DuckDuckGo".to_string(),
                message: format!("HTTP {}", response.status()),
            });
        }

        Ok(response.text().await?)
    }

    fn parse_results(&self, html: &str) -> Result<Vec<DuckDuckGoResult>> {
        let document = scraper::Html::parse_document(html);
        let selector = scraper::Selector::parse(".result")
            .expect("valid CSS selector: .result");
        let title_selector = scraper::Selector::parse(".result__title a")
            .expect("valid CSS selector: .result__title a");
        let snippet_selector = scraper::Selector::parse(".result__snippet")
            .expect("valid CSS selector: .result__snippet");
        let url_selector = scraper::Selector::parse(".result__url")
            .expect("valid CSS selector: .result__url");

        let mut results = Vec::new();

        for element in document.select(&selector) {
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let snippet = element
                .select(&snippet_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let url = element
                .select(&title_selector)
                .next()
                .and_then(|e| e.value().attr("href"))
                .map(|s| s.to_string())
                .or_else(|| {
                    element
                        .select(&url_selector)
                        .next()
                        .map(|e| e.text().collect::<String>().trim().to_string())
                })
                .unwrap_or_default();

            if !title.is_empty() && !url.is_empty() {
                results.push(DuckDuckGoResult {
                    title,
                    url,
                    snippet,
                });
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for DuckDuckGoConnector {
    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let start = std::time::Instant::now();

        let html = self.fetch_html(&query.query).await?;
        let parsed = self.parse_results(&html)?;

        let documents: Vec<ExternalDocument> = parsed
            .into_iter()
            .take(query.limit)
            .enumerate()
            .map(|(idx, result)| ExternalDocument {
                id: format!("ddg:{}", idx),
                title: result.title,
                content: result.snippet,
                url: result.url,
                source: "DuckDuckGo".to_string(),
                source_type: SourceType::SearchEngine,
                published_at: None,
                authors: vec![],
                tags: vec!["web-search".to_string()],
                metadata: serde_json::json!({
                    "rank": idx + 1,
                    "engine": "DuckDuckGo",
                }),
                confidence: 0.60, // Web search results have moderate confidence
                fetched_at: Utc::now(),
            })
            .collect();

        let search_time = start.elapsed().as_millis() as u64;

        Ok(SearchResult {
            total_count: documents.len(),
            documents,
            query: query.clone(),
            search_time_ms: search_time,
        })
    }

    fn name(&self) -> &'static str {
        "DuckDuckGo"
    }
}

impl Default for DuckDuckGoConnector {
    fn default() -> Self {
        Self::new().expect("valid default HTTP client config")
    }
}

/// SerpAPI-based search connector (for Google, Bing, etc.)
/// Requires API key
pub struct SerpApiConnector {
    client: reqwest::Client,
    api_key: String,
    engine: String,
    config: ConnectorConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct SerpApiResponse {
    #[serde(rename = "organic_results")]
    organic_results: Option<Vec<SerpApiResult>>,
    #[serde(rename = "search_information")]
    search_info: Option<SerpApiSearchInfo>,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SerpApiResult {
    title: String,
    link: String,
    snippet: Option<String>,
    #[serde(rename = "displayed_link")]
    _displayed_link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SerpApiSearchInfo {
    #[serde(rename = "total_results")]
    total_results: Option<String>,
}

impl SerpApiConnector {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::with_engine(api_key, "google")
    }

    pub fn with_engine(api_key: impl Into<String>, engine: impl Into<String>) -> Result<Self> {
        let config = ConnectorConfig::default();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|e| ExternalError::NetworkError(format!("HTTP client: {}", e)))?;

        Ok(Self {
            client,
            api_key: api_key.into(),
            engine: engine.into(),
            config,
        })
    }

    pub fn with_config(mut self, config: ConnectorConfig) -> Self {
        self.config = config;
        self
    }
}

#[async_trait]
impl SearchEngine for SerpApiConnector {
    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let start = std::time::Instant::now();

        let url = "https://serpapi.com/search".to_string();

        let params = [
            ("q", query.query.as_str()),
            ("engine", self.engine.as_str()),
            ("api_key", self.api_key.as_str()),
            ("num", &query.limit.min(100).to_string()),
        ];

        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ExternalError::ApiError {
                provider: "SerpAPI".to_string(),
                message: format!("HTTP {}: {}", status, text),
            });
        }

        let data: SerpApiResponse = response.json().await?;

        if let Some(error) = data.error {
            return Err(ExternalError::ApiError {
                provider: "SerpAPI".to_string(),
                message: error,
            });
        }

        let results = data.organic_results.unwrap_or_default();
        let total = data
            .search_info
            .and_then(|info| info.total_results)
            .and_then(|s| s.replace(",", "").parse().ok())
            .unwrap_or(results.len());

        let documents: Vec<ExternalDocument> = results
            .into_iter()
            .enumerate()
            .map(|(idx, result)| ExternalDocument {
                id: format!("serp:{}:{}", self.engine, idx),
                title: result.title,
                content: result.snippet.unwrap_or_default(),
                url: result.link,
                source: format!("SerpAPI-{}", self.engine),
                source_type: SourceType::SearchEngine,
                published_at: None,
                authors: vec![],
                tags: vec!["search-result".to_string()],
                metadata: serde_json::json!({
                    "rank": idx + 1,
                    "engine": self.engine,
                }),
                confidence: 0.65,
                fetched_at: Utc::now(),
            })
            .collect();

        let search_time = start.elapsed().as_millis() as u64;

        Ok(SearchResult {
            total_count: total,
            documents,
            query: query.clone(),
            search_time_ms: search_time,
        })
    }

    fn name(&self) -> &'static str {
        "SerpAPI"
    }
}

/// Multi-engine search aggregator
pub struct AggregatedSearchEngine {
    engines: Vec<Box<dyn SearchEngine>>,
}

impl AggregatedSearchEngine {
    pub fn new() -> Self {
        Self {
            engines: Vec::new(),
        }
    }

    pub fn add_engine(&mut self, engine: Box<dyn SearchEngine>) {
        self.engines.push(engine);
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        use futures::future::join_all;

        let futures: Vec<_> = self
            .engines
            .iter()
            .map(|engine| engine.search(query))
            .collect();

        let results = join_all(futures).await;

        let mut successes = Vec::new();
        for result in results {
            match result {
                Ok(r) => successes.push(r),
                Err(e) => {
                    warn!("Search engine failed: {}", e);
                }
            }
        }

        Ok(successes)
    }

    /// Deduplicate and merge results from multiple engines
    pub async fn search_merged(&self, query: &SearchQuery) -> Result<SearchResult> {
        let start = std::time::Instant::now();
        let results = self.search(query).await?;

        let mut seen_urls = std::collections::HashSet::new();
        let mut merged_docs = Vec::new();

        for result in results {
            for doc in result.documents {
                if seen_urls.insert(doc.url.clone()) {
                    merged_docs.push(doc);
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

impl Default for AggregatedSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

// URL encoding helper for DuckDuckGo
fn urlencode(s: &str) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}
