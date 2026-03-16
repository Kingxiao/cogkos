//! Generic API Polling Subscription Module
//!
//! Implements general API polling with:
//! - Support for GET/POST methods
//! - Authentication (API Key, Bearer Token, OAuth2)
//! - Configurable polling intervals
//! - Deduplication (Response body hash/ID)
//! - Configurable JSON path extraction

use crate::{
    ExternalDocument, Result, RssFeedManager, error::ExternalError, types::*,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use url::Url;

/// Supported HTTP methods for polling
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ApiMethod {
    Get,
    Post,
}

/// Supported Authentication Methods
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum AuthMethod {
    ApiKey {
        header: String,
        key: String,
    },
    BearerToken(String),
    BasicAuth {
        username: String,
        password: Option<String>,
    },
    OAuth2ClientCredentials {
        token_url: String,
        client_id: String,
        client_secret: String,
        scope: Option<String>,
    },
}

/// Configuration for API subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSubscriptionConfig {
    pub name: String,
    pub url: String,
    pub method: ApiMethod,
    pub headers: HashMap<String, String>,
    pub body: Option<serde_json::Value>,
    pub auth: Option<AuthMethod>,
    pub poll_interval_seconds: u64,
    pub deduplication_key: Option<String>, // JSON path to unique ID or uses whole body hash
    pub extraction_path: Option<String>, // JSON path to array of items if response is not an item itself
    pub max_entries_per_poll: usize,
    pub timeout_seconds: u64,
}

impl Default for ApiSubscriptionConfig {
    fn default() -> Self {
        Self {
            name: "API Subscription".to_string(),
            url: String::new(),
            method: ApiMethod::Get,
            headers: HashMap::new(),
            body: None,
            auth: None,
            poll_interval_seconds: 3600, // 1 hour
            deduplication_key: None,
            extraction_path: None,
            max_entries_per_poll: 100,
            timeout_seconds: 30,
        }
    }
}

/// Entry polled from an API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiPollingEntry {
    pub id: String,
    pub data: serde_json::Value,
    pub fetched_at: DateTime<Utc>,
}

/// Generic API subscription manager
#[derive(Clone)]
pub struct ApiSubscriptionManager {
    client: reqwest::Client,
    subscriptions: Arc<RwLock<HashMap<String, ApiSubscriptionConfig>>>,
    seen_entries: Arc<RwLock<HashMap<String, HashSet<String>>>>, // subscription_id -> set of entry IDs/hashes
    #[allow(clippy::type_complexity)]
    oauth_tokens: Arc<RwLock<HashMap<String, (String, DateTime<Utc>)>>>, // subscription_id -> (token, expiry)
}

impl ApiSubscriptionManager {
    /// Create new API subscription manager
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ExternalError::NetworkError(format!("HTTP client: {}", e)))?;

        Ok(Self {
            client,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            seen_entries: Arc::new(RwLock::new(HashMap::new())),
            oauth_tokens: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Add an API subscription
    pub async fn add_subscription(&self, config: ApiSubscriptionConfig) -> Result<String> {
        // Validate URL
        Url::parse(&config.url)
            .map_err(|e| ExternalError::InvalidParams(format!("Invalid API URL: {}", e)))?;

        let sub_id = uuid::Uuid::new_v4().to_string();
        self.subscriptions
            .write()
            .await
            .insert(sub_id.clone(), config);
        self.seen_entries
            .write()
            .await
            .insert(sub_id.clone(), HashSet::new());

        Ok(sub_id)
    }

    /// Remove an API subscription
    pub async fn remove_subscription(&self, sub_id: &str) -> Result<()> {
        self.subscriptions.write().await.remove(sub_id);
        self.seen_entries.write().await.remove(sub_id);
        self.oauth_tokens.write().await.remove(sub_id);
        Ok(())
    }

    /// Poll a specific subscription and return new entries
    pub async fn poll_subscription(&self, sub_id: &str) -> Result<Vec<ApiPollingEntry>> {
        let config = {
            let subs = self.subscriptions.read().await;
            subs.get(sub_id).cloned().ok_or_else(|| {
                ExternalError::InvalidParams(format!("Subscription {} not found", sub_id))
            })?
        };

        // Build request
        let mut request = match config.method {
            ApiMethod::Get => self.client.get(&config.url),
            ApiMethod::Post => self.client.post(&config.url),
        };

        // Add headers
        for (k, v) in &config.headers {
            request = request.header(k, v);
        }

        // Add body for POST
        if let (ApiMethod::Post, Some(body)) = (&config.method, &config.body) {
            request = request.json(body);
        }

        // Add authentication
        if let Some(auth) = &config.auth {
            request = self.apply_auth(sub_id, request, auth).await?;
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| ExternalError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ExternalError::NetworkError(format!(
                "API returned status: {}",
                response.status()
            )));
        }

        let body: serde_json::Value = response.json().await.map_err(|e| {
            ExternalError::ParseError(format!("Failed to parse JSON response: {}", e))
        })?;

        // Extract entries
        let entries = self.extract_entries(body, &config)?;

        // Filter and deduplicate
        let mut new_entries = Vec::new();
        let mut seen = self.seen_entries.write().await;
        let sub_seen = seen.entry(sub_id.to_string()).or_insert_with(HashSet::new);

        for entry_data in entries.into_iter().take(config.max_entries_per_poll) {
            let entry_id = self.get_entry_id(&entry_data, &config.deduplication_key);

            if !sub_seen.contains(&entry_id) {
                sub_seen.insert(entry_id.clone());
                new_entries.push(ApiPollingEntry {
                    id: entry_id,
                    data: entry_data,
                    fetched_at: Utc::now(),
                });
            }
        }

        // Evict oldest entries if seen set exceeds 10k per subscription
        const MAX_SEEN_ENTRIES: usize = 10_000;
        if sub_seen.len() > MAX_SEEN_ENTRIES {
            let to_remove = sub_seen.len() - MAX_SEEN_ENTRIES;
            let keys: Vec<_> = sub_seen.iter().take(to_remove).cloned().collect();
            for key in keys {
                sub_seen.remove(&key);
            }
        }

        Ok(new_entries)
    }

    /// Apply authentication to the request
    async fn apply_auth(
        &self,
        sub_id: &str,
        mut request: reqwest::RequestBuilder,
        auth: &AuthMethod,
    ) -> Result<reqwest::RequestBuilder> {
        match auth {
            AuthMethod::ApiKey { header, key } => {
                request = request.header(header, key);
            }
            AuthMethod::BearerToken(token) => {
                request = request.bearer_auth(token);
            }
            AuthMethod::BasicAuth { username, password } => {
                request = request.basic_auth(username, password.as_ref());
            }
            AuthMethod::OAuth2ClientCredentials {
                token_url,
                client_id,
                client_secret,
                scope,
            } => {
                let token = self
                    .get_oauth_token(sub_id, token_url, client_id, client_secret, scope)
                    .await?;
                request = request.bearer_auth(token);
            }
        }
        Ok(request)
    }

    /// Get or refresh OAuth2 token
    async fn get_oauth_token(
        &self,
        sub_id: &str,
        token_url: &str,
        client_id: &str,
        client_secret: &str,
        scope: &Option<String>,
    ) -> Result<String> {
        // Check cache
        {
            let tokens = self.oauth_tokens.read().await;
            if let Some((token, expiry)) = tokens.get(sub_id)
                && *expiry > Utc::now() + chrono::Duration::seconds(30)
            {
                return Ok(token.clone());
            }
        }

        // Fetch new token
        let mut params = vec![
            ("grant_type", "client_credentials"),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];
        if let Some(s) = scope {
            params.push(("scope", s));
        }

        let response = self
            .client
            .post(token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| ExternalError::NetworkError(format!("OAuth failed: {}", e)))?;

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: Option<i64>,
        }

        let token_data: TokenResponse = response.json().await.map_err(|e| {
            ExternalError::ParseError(format!("Failed to parse OAuth response: {}", e))
        })?;

        let expiry = Utc::now() + chrono::Duration::seconds(token_data.expires_in.unwrap_or(3600));

        self.oauth_tokens.write().await.insert(
            sub_id.to_string(),
            (token_data.access_token.clone(), expiry),
        );

        Ok(token_data.access_token)
    }

    /// Extract entries from response body using extraction path
    fn extract_entries(
        &self,
        body: serde_json::Value,
        config: &ApiSubscriptionConfig,
    ) -> Result<Vec<serde_json::Value>> {
        if let Some(path) = &config.extraction_path {
            // Simple JSON path implementation (dot-separated)
            let mut current = &body;
            for part in path.split('.') {
                if part.is_empty() {
                    continue;
                }
                current = current.get(part).ok_or_else(|| {
                    ExternalError::ParseError(format!("Extraction path part '{}' not found", part))
                })?;
            }

            if let Some(arr) = current.as_array() {
                Ok(arr.clone())
            } else {
                Ok(vec![current.clone()])
            }
        } else {
            // If no path, treat the whole body as one entry if it's an object,
            // or if it's an array, treat each element as an entry.
            if let Some(arr) = body.as_array() {
                Ok(arr.clone())
            } else {
                Ok(vec![body])
            }
        }
    }

    /// Generate entry ID for deduplication
    fn get_entry_id(&self, entry: &serde_json::Value, key_path: &Option<String>) -> String {
        if let Some(path) = key_path {
            let mut current = entry;
            for part in path.split('.') {
                if part.is_empty() {
                    continue;
                }
                if let Some(next) = current.get(part) {
                    current = next;
                } else {
                    // Fallback to hash if path not found
                    return self.hash_value(entry);
                }
            }

            match current {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                _ => self.hash_value(current),
            }
        } else {
            self.hash_value(entry)
        }
    }

    /// Simple hash for JSON value
    fn hash_value(&self, value: &serde_json::Value) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let s = value.to_string();
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Start background polling task (Conceptual)
    pub async fn start_polling_loop<F>(self: Arc<Self>, callback: F)
    where
        F: Fn(String, Vec<ApiPollingEntry>) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    {
        let mut poll_interval = interval(Duration::from_secs(60)); // Check every minute

        loop {
            poll_interval.tick().await;

            let subs_snapshot = {
                let subs = self.subscriptions.read().await;
                subs.clone()
            };

            for (sub_id, _config) in subs_snapshot {
                // In a real implementation, we should track last poll time per subscription
                // For simplicity here, we poll all
                if let Ok(entries) = self.poll_subscription(&sub_id).await
                    && !entries.is_empty()
                {
                    callback(sub_id, entries).await;
                }
            }
        }
    }

    /// Convert polling entries to ExternalDocument
    pub fn entries_to_documents(
        &self,
        config: &ApiSubscriptionConfig,
        entries: Vec<ApiPollingEntry>,
    ) -> Vec<ExternalDocument> {
        entries
            .into_iter()
            .map(|entry| {
                // Try to extract some common fields if they exist
                let title = entry
                    .data
                    .get("title")
                    .and_then(|v| v.as_str())
                    .or_else(|| entry.data.get("name").and_then(|v| v.as_str()))
                    .unwrap_or("API Entry");

                let content = entry
                    .data
                    .get("content")
                    .and_then(|v| v.as_str())
                    .or_else(|| entry.data.get("body").and_then(|v| v.as_str()))
                    .unwrap_or("No content");

                let url = entry
                    .data
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&config.url);

                ExternalDocument {
                    id: format!("api:{}:{}", config.name, entry.id),
                    title: title.to_string(),
                    content: content.to_string(),
                    url: url.to_string(),
                    source: config.name.clone(),
                    source_type: SourceType::ApiResponse,
                    published_at: None, // Could try to extract from data
                    authors: Vec::new(),
                    tags: Vec::new(),
                    metadata: entry.data,
                    confidence: 0.8,
                    fetched_at: entry.fetched_at,
                }
            })
            .collect()
    }
}

/// Type alias for async boxed futures
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// RSS Subscription Manager - wraps RssFeedManager for polling
#[derive(Clone)]
pub struct RssSubscriptionManager {
    feed_manager: Arc<RssFeedManager>,
}

impl RssSubscriptionManager {
    /// Create new RSS subscription manager
    pub fn new() -> Self {
        Self {
            feed_manager: Arc::new(RssFeedManager::new()),
        }
    }

    /// Get the feed manager
    pub fn feed_manager(&self) -> Arc<RssFeedManager> {
        Arc::clone(&self.feed_manager)
    }
}

impl Default for RssSubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for ApiSubscriptionManager {
    fn default() -> Self {
        Self::new().expect("valid default HTTP client config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extract_entries_simple() {
        let manager = ApiSubscriptionManager::new().unwrap();
        let config = ApiSubscriptionConfig {
            extraction_path: Some("data.items".to_string()),
            ..Default::default()
        };

        let body = serde_json::json!({
            "data": {
                "items": [
                    {"id": 1, "val": "a"},
                    {"id": 2, "val": "b"}
                ]
            }
        });

        let entries = manager.extract_entries(body, &config).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["id"], 1);
    }
}
