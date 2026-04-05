use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalDocument {
    pub id: String,
    pub title: String,
    pub content: String,
    pub url: String,
    pub source: String,
    pub source_type: SourceType,
    pub published_at: Option<DateTime<Utc>>,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
    pub confidence: f64,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Wikipedia,
    Arxiv,
    SearchEngine,
    RssFeed,
    WebPage,
    ApiResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchQuery {
    pub query: String,
    pub filters: Vec<SearchFilter>,
    pub limit: usize,
    pub offset: usize,
    pub sort_by: SortOption,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum SearchFilter {
    DateRange {
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    },
    Source(Vec<String>),
    Author(String),
    Tag(String),
    Language(String),
    MinConfidence(f64),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SortOption {
    Relevance,
    DateDesc,
    DateAsc,
    Confidence,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            filters: Vec::new(),
            limit: 10,
            offset: 0,
            sort_by: SortOption::Relevance,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub documents: Vec<ExternalDocument>,
    pub total_count: usize,
    pub query: SearchQuery,
    pub search_time_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectorConfig {
    pub timeout_seconds: u64,
    pub max_results: usize,
    pub cache_ttl_seconds: u64,
    pub rate_limit_per_minute: u32,
    pub retry_attempts: u32,
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            max_results: 50,
            cache_ttl_seconds: 3600,
            rate_limit_per_minute: 60,
            retry_attempts: 3,
        }
    }
}
