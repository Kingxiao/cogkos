use crate::error::{ExternalError, Result};
use crate::types::{ConnectorConfig, ExternalDocument, SearchQuery, SearchResult, SourceType};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{debug, error};

/// arXiv API Connector
/// Uses the arXiv API (http://export.arxiv.org/api/)
pub struct ArxivConnector {
    client: reqwest::Client,
    base_url: String,
    config: ConnectorConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ArxivFeed {
    #[serde(rename = "entry")]
    entries: Option<Vec<ArxivEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArxivEntry {
    id: String,
    pub title: String,
    summary: String,
    #[serde(rename = "published")]
    published: String,
    #[serde(rename = "updated")]
    updated: Option<String>,
    author: Vec<ArxivAuthor>,
    #[serde(rename = "category")]
    categories: Option<Vec<ArxivCategory>>,
    #[serde(rename = "link")]
    links: Option<Vec<ArxivLink>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ArxivAuthor {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ArxivCategory {
    #[serde(rename = "term")]
    term: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ArxivLink {
    #[serde(rename = "href")]
    href: String,
    #[serde(rename = "type")]
    content_type: Option<String>,
    #[serde(rename = "title")]
    _title: Option<String>,
}

impl ArxivConnector {
    pub fn new() -> Result<Self> {
        Self::with_config(ConnectorConfig::default())
    }

    pub fn with_config(config: ConnectorConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .user_agent("CogKOS-ArxivBot/1.0")
            .build()
            .map_err(|e| ExternalError::NetworkError(format!("HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: "http://export.arxiv.org/api/query".to_string(),
            config,
        })
    }

    /// Search arXiv for papers
    ///
    /// Query format supports:
    /// - ti: Title
    /// - au: Author
    /// - abs: Abstract
    /// - cat: Category
    /// - all: All fields
    pub async fn search(
        &self,
        query: &str,
        start: usize,
        max_results: usize,
    ) -> Result<Vec<ArxivEntry>> {
        let max_results = max_results.min(self.config.max_results);

        let params = [
            ("search_query", query),
            ("start", &start.to_string()),
            ("max_results", &max_results.to_string()),
            ("sortBy", "relevance"),
            ("sortOrder", "descending"),
        ];

        debug!("Searching arXiv for: {}", query);

        let response = self
            .client
            .get(&self.base_url)
            .query(&params)
            .send()
            .await?;

        if response.status().as_u16() == 429 {
            return Err(ExternalError::RateLimited(3)); // arXiv rate limit is 3 seconds
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ExternalError::ApiError {
                provider: "arXiv".to_string(),
                message: format!("HTTP {}: {}", status, text),
            });
        }

        // arXiv returns Atom XML, we need to parse it
        let xml_text = response.text().await?;
        let entries = self.parse_atom_feed(&xml_text)?;

        Ok(entries)
    }

    /// Search by category
    pub async fn search_by_category(
        &self,
        category: &str,
        max_results: usize,
    ) -> Result<Vec<ArxivEntry>> {
        let query = format!("cat:{}", category);
        self.search(&query, 0, max_results).await
    }

    /// Search by author
    pub async fn search_by_author(
        &self,
        author: &str,
        max_results: usize,
    ) -> Result<Vec<ArxivEntry>> {
        let query = format!("au:{}\"{}\"", "%22", author);
        self.search(&query, 0, max_results).await
    }

    /// Search by title
    pub async fn search_by_title(
        &self,
        title: &str,
        max_results: usize,
    ) -> Result<Vec<ArxivEntry>> {
        let query = format!("ti:{}\"{}\"", "%22", title);
        self.search(&query, 0, max_results).await
    }

    /// Get paper by ID
    pub async fn get_paper(&self, arxiv_id: &str) -> Result<ExternalDocument> {
        let _query = format!("id_list={}", arxiv_id);

        let response = self
            .client
            .get(&self.base_url)
            .query(&[("id_list", arxiv_id)])
            .send()
            .await?;

        let xml_text = response.text().await?;
        let entries = self.parse_atom_feed(&xml_text)?;

        let entry = entries
            .into_iter()
            .next()
            .ok_or_else(|| ExternalError::NotFound(format!("Paper not found: {}", arxiv_id)))?;

        self.entry_to_document(entry).await
    }

    /// Search and return as ExternalDocuments
    pub async fn search_documents(&self, search_query: &SearchQuery) -> Result<SearchResult> {
        let start_time = std::time::Instant::now();

        // Build arXiv query from SearchQuery
        let query = self.build_query(search_query);

        let entries = self
            .search(&query, search_query.offset, search_query.limit)
            .await?;

        let mut documents = Vec::new();
        for entry in entries {
            match self.entry_to_document(entry).await {
                Ok(doc) => documents.push(doc),
                Err(e) => {
                    error!("Failed to convert entry to document: {}", e);
                }
            }
        }

        let search_time = start_time.elapsed().as_millis() as u64;

        Ok(SearchResult {
            total_count: documents.len(),
            documents,
            query: search_query.clone(),
            search_time_ms: search_time,
        })
    }

    fn build_query(&self, search_query: &SearchQuery) -> String {
        let mut parts = Vec::new();

        // Add main query
        if !search_query.query.is_empty() {
            parts.push(format!("all:{}", search_query.query));
        }

        // Add filters
        for filter in &search_query.filters {
            match filter {
                crate::types::SearchFilter::DateRange { from: _, to: _ } => {
                    // arXiv doesn't support date range in query, handled separately
                }
                crate::types::SearchFilter::Author(author) => {
                    parts.push(format!("au:\"{}\"", author));
                }
                crate::types::SearchFilter::Tag(category) => {
                    parts.push(format!("cat:{}", category));
                }
                _ => {}
            }
        }

        if parts.is_empty() {
            "*".to_string()
        } else {
            parts.join(" AND ")
        }
    }

    async fn entry_to_document(&self, entry: ArxivEntry) -> Result<ExternalDocument> {
        let authors: Vec<String> = entry.author.iter().map(|a| a.name.clone()).collect();

        let categories: Vec<String> = entry
            .categories
            .as_ref()
            .map(|cats| cats.iter().map(|c| c.term.clone()).collect())
            .unwrap_or_default();

        let pdf_url = entry
            .links
            .as_ref()
            .and_then(|links| {
                links
                    .iter()
                    .find(|l| l.content_type.as_deref() == Some("application/pdf"))
                    .map(|l| l.href.clone())
            })
            .unwrap_or_else(|| entry.id.clone());

        let published = self.parse_arxiv_date(&entry.published)?;

        Ok(ExternalDocument {
            id: entry.id.clone(),
            title: entry.title.trim().to_string(),
            content: entry.summary.trim().to_string(),
            url: pdf_url,
            source: "arXiv".to_string(),
            source_type: SourceType::Arxiv,
            published_at: Some(published),
            authors,
            tags: categories,
            metadata: serde_json::json!({
                "arxiv_id": entry.id,
                "updated": entry.updated,
            }),
            confidence: 0.90, // Academic papers have high credibility
            fetched_at: Utc::now(),
        })
    }

    fn parse_atom_feed(&self, xml: &str) -> Result<Vec<ArxivEntry>> {
        // Simple XML parsing - in production use a proper XML parser
        let mut entries = Vec::new();

        // Check if there are no results
        if xml.contains("<opensearch:totalResults>0</opensearch:totalResults>") {
            return Ok(entries);
        }

        // Parse entries - simplified version
        // NOTE: In production, use quick-xml or roxmltree
        let entry_regex = regex::Regex::new(r"<entry[^>]*>(.*?)</entry>")
            .map_err(|e| ExternalError::ParseError(e.to_string()))?;

        for cap in entry_regex.captures_iter(xml) {
            if let Some(entry_xml) = cap.get(1)
                && let Ok(entry) = self.parse_entry(entry_xml.as_str())
            {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    fn parse_entry(&self, xml: &str) -> Result<ArxivEntry> {
        // Extract fields using simple regex - in production use proper XML parsing
        let title = self.extract_xml_tag(xml, "title").unwrap_or_default();
        let id = self.extract_xml_tag(xml, "id").unwrap_or_default();
        let summary = self.extract_xml_tag(xml, "summary").unwrap_or_default();
        let published = self.extract_xml_tag(xml, "published").unwrap_or_default();
        let updated = self.extract_xml_tag(xml, "updated");

        // Parse authors
        let author_regex = regex::Regex::new(r"<author[^>]*>.*?<name>(.*?)</name>.*?</author>")
            .map_err(|e| ExternalError::ParseError(e.to_string()))?;

        let authors: Vec<ArxivAuthor> = author_regex
            .captures_iter(xml)
            .filter_map(|cap| cap.get(1))
            .map(|m| ArxivAuthor {
                name: m.as_str().to_string(),
            })
            .collect();

        Ok(ArxivEntry {
            id,
            title,
            summary,
            published,
            updated,
            author: authors,
            categories: None,
            links: None,
        })
    }

    fn extract_xml_tag(&self, xml: &str, tag: &str) -> Option<String> {
        let pattern = format!("<{}[^>]*>(.*?)</{}>", tag, tag);
        let regex = regex::Regex::new(&pattern).ok()?;
        regex.captures(xml)?.get(1).map(|m| {
            let text = m.as_str();
            // Simple HTML entity decoding
            text.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .to_string()
        })
    }

    fn parse_arxiv_date(&self, date_str: &str) -> Result<DateTime<Utc>> {
        // arXiv dates are in RFC 3339 format
        DateTime::parse_from_rfc3339(date_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| ExternalError::ParseError(format!("Invalid date: {}", e)))
    }
}

impl Default for ArxivConnector {
    fn default() -> Self {
        Self::new().expect("valid default HTTP client config")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ArxivSearchResponse {
    #[serde(rename = "feed")]
    feed: ArxivFeed,
}
