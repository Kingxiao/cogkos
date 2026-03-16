use crate::error::{ExternalError, Result};
use crate::types::{ConnectorConfig, ExternalDocument, SearchQuery, SearchResult, SourceType};
use chrono::Utc;
use serde::Deserialize;
use tracing::debug;

/// Wikipedia API Client
/// Uses the MediaWiki Action API
pub struct WikipediaConnector {
    client: reqwest::Client,
    base_url: String,
    config: ConnectorConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct WikipediaSearchResponse {
    query: WikipediaQuery,
}

#[derive(Debug, Clone, Deserialize)]
struct WikipediaQuery {
    search: Vec<WikipediaSearchResult>,
    #[serde(rename = "pageids")]
    _page_ids: Option<Vec<String>>,
    pages: Option<std::collections::HashMap<String, WikipediaPage>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WikipediaSearchResult {
    #[serde(rename = "pageid")]
    page_id: i64,
    title: String,
    snippet: String,
    #[serde(rename = "timestamp")]
    timestamp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WikipediaPage {
    #[serde(rename = "pageid")]
    page_id: i64,
    title: String,
    extract: Option<String>,
    _content: Option<String>,
    #[serde(rename = "fullurl")]
    full_url: Option<String>,
    #[serde(rename = "canonicalurl")]
    canonical_url: Option<String>,
}

impl WikipediaConnector {
    pub fn new() -> Result<Self> {
        Self::with_config(ConnectorConfig::default())
    }

    pub fn with_config(config: ConnectorConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .user_agent("CogKOS-WikipediaBot/1.0")
            .build()
            .map_err(|e| ExternalError::NetworkError(format!("HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: "https://en.wikipedia.org/w/api.php".to_string(),
            config,
        })
    }

    pub fn with_language(mut self, lang: &str) -> Self {
        self.base_url = format!("https://{}.wikipedia.org/w/api.php", lang);
        self
    }

    /// Search Wikipedia for articles matching the query
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<WikipediaSearchResult>> {
        let limit = limit.min(self.config.max_results);

        let params = [
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("srlimit", &limit.to_string()),
            ("format", "json"),
            ("origin", "*"),
        ];

        debug!("Searching Wikipedia for: {}", query);

        let response = self
            .client
            .get(&self.base_url)
            .query(&params)
            .send()
            .await?;

        if response.status().as_u16() == 429 {
            return Err(ExternalError::RateLimited(60));
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ExternalError::ApiError {
                provider: "Wikipedia".to_string(),
                message: format!("HTTP {}: {}", status, text),
            });
        }

        let data: WikipediaSearchResponse = response.json().await?;
        Ok(data.query.search)
    }

    /// Get the full content of a Wikipedia page
    pub async fn get_page(&self, title: &str) -> Result<ExternalDocument> {
        let params = [
            ("action", "query"),
            ("titles", title),
            ("prop", "extracts|info"),
            ("exintro", "false"),
            ("explaintext", "true"),
            ("exlimit", "1"),
            ("inprop", "url"),
            ("format", "json"),
            ("origin", "*"),
        ];

        debug!("Fetching Wikipedia page: {}", title);

        let response = self
            .client
            .get(&self.base_url)
            .query(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(ExternalError::ApiError {
                provider: "Wikipedia".to_string(),
                message: format!("HTTP {}", status),
            });
        }

        let data: WikipediaSearchResponse = response.json().await?;

        let pages = data
            .query
            .pages
            .ok_or_else(|| ExternalError::NotFound(format!("Page not found: {}", title)))?;

        let page = pages
            .values()
            .next()
            .ok_or_else(|| ExternalError::NotFound(format!("Page not found: {}", title)))?;

        let url = page
            .full_url
            .clone()
            .or_else(|| page.canonical_url.clone())
            .unwrap_or_else(|| {
                format!("https://en.wikipedia.org/wiki/{}", title.replace(" ", "_"))
            });

        let content = page
            .extract
            .clone()
            .unwrap_or_else(|| format!("Article: {}", page.title));

        Ok(ExternalDocument {
            id: format!("wikipedia:{}", page.page_id),
            title: page.title.clone(),
            content,
            url,
            source: "Wikipedia".to_string(),
            source_type: SourceType::Wikipedia,
            published_at: None,
            authors: vec!["Wikipedia Contributors".to_string()],
            tags: vec!["encyclopedia".to_string()],
            metadata: serde_json::json!({
                "page_id": page.page_id,
                "language": self.extract_language(),
            }),
            confidence: 0.85,
            fetched_at: Utc::now(),
        })
    }

    /// Get a summary of a Wikipedia page
    pub async fn get_summary(&self, title: &str) -> Result<String> {
        let params = [
            ("action", "query"),
            ("titles", title),
            ("prop", "extracts"),
            ("exintro", "true"),
            ("explaintext", "true"),
            ("format", "json"),
            ("origin", "*"),
        ];

        let response = self
            .client
            .get(&self.base_url)
            .query(&params)
            .send()
            .await?;

        let data: WikipediaSearchResponse = response.json().await?;

        if let Some(pages) = data.query.pages
            && let Some(page) = pages.values().next()
        {
            return Ok(page.extract.clone().unwrap_or_default());
        }

        Err(ExternalError::NotFound(format!(
            "Summary not found for: {}",
            title
        )))
    }

    /// Search and convert to ExternalDocument format
    pub async fn search_documents(&self, query: &SearchQuery) -> Result<SearchResult> {
        let start = std::time::Instant::now();

        let search_results = self.search(&query.query, query.limit).await?;

        let mut documents = Vec::new();
        for result in search_results {
            let clean_snippet = html_escape::decode_html_entities(&result.snippet).to_string();

            let url = format!(
                "https://en.wikipedia.org/wiki/{}",
                result.title.replace(" ", "_")
            );

            documents.push(ExternalDocument {
                id: format!("wikipedia:{}", result.page_id),
                title: result.title.clone(),
                content: clean_snippet,
                url,
                source: "Wikipedia".to_string(),
                source_type: SourceType::Wikipedia,
                published_at: result
                    .timestamp
                    .as_ref()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&Utc)),
                authors: vec!["Wikipedia Contributors".to_string()],
                tags: vec!["search-result".to_string()],
                metadata: serde_json::json!({
                    "page_id": result.page_id,
                    "snippet": true,
                }),
                confidence: 0.75,
                fetched_at: Utc::now(),
            });
        }

        let search_time = start.elapsed().as_millis() as u64;

        Ok(SearchResult {
            total_count: documents.len(),
            documents,
            query: query.clone(),
            search_time_ms: search_time,
        })
    }

    fn extract_language(&self) -> String {
        self.base_url
            .split("//")
            .nth(1)
            .and_then(|s| s.split('.').next())
            .unwrap_or("en")
            .to_string()
    }
}

impl Default for WikipediaConnector {
    fn default() -> Self {
        Self::new().expect("valid default HTTP client config")
    }
}
