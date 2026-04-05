//! External knowledge polling integration
//!
//! Periodically fetches RSS feeds and API endpoints, converts results to
//! EpistemicClaims, and ingests them into the knowledge store.

use cogkos_core::models::{
    AccessEnvelope, Claimant, EpistemicClaim, KnowledgeType, NodeType, ProvenanceRecord,
};
use cogkos_external::{ExternalDocument, RssFeedConfig, RssFeedManager};
use cogkos_store::Stores;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Configuration for external knowledge polling
#[derive(Debug, Clone)]
pub struct PollingConfig {
    /// RSS feeds to subscribe to (loaded from environment/config)
    pub rss_feeds: Vec<RssFeedConfig>,
    /// Default tenant for ingested claims
    pub tenant_id: String,
    /// Polling interval in seconds (how often we check all feeds)
    pub poll_interval_secs: u64,
    /// Maximum items to ingest per poll cycle
    pub max_items_per_cycle: usize,
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            rss_feeds: Vec::new(),
            tenant_id: "default".to_string(),
            poll_interval_secs: 3600, // 1 hour
            max_items_per_cycle: 100,
        }
    }
}

impl PollingConfig {
    /// Load from environment variables
    ///
    /// Format: RSS_FEEDS="url1|name1,url2|name2"
    /// Example: RSS_FEEDS="https://hnrss.org/newest|hackernews,https://arxiv.org/rss/cs.AI|arxiv-ai"
    pub fn from_env() -> Self {
        let tenant_id = std::env::var("POLLING_TENANT_ID").unwrap_or("default".to_string());
        let poll_interval = std::env::var("POLLING_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);
        let max_items = std::env::var("POLLING_MAX_ITEMS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let rss_feeds = std::env::var("RSS_FEEDS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .filter_map(|entry| {
                let parts: Vec<&str> = entry.split('|').collect();
                if parts.len() >= 1 {
                    Some(RssFeedConfig {
                        url: parts[0].to_string(),
                        poll_interval_secs: poll_interval,
                        max_items: 20,
                        fetch_full_content: false,
                        headers: Default::default(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Self {
            rss_feeds,
            tenant_id,
            poll_interval_secs: poll_interval,
            max_items_per_cycle: max_items,
        }
    }
}

/// Start the external knowledge polling background task
pub async fn start_polling(stores: Arc<Stores>, config: PollingConfig, cancel: CancellationToken) {
    if config.rss_feeds.is_empty() {
        info!("No RSS feeds configured (set RSS_FEEDS env var), external polling disabled");
        return;
    }

    info!(
        feed_count = config.rss_feeds.len(),
        interval_secs = config.poll_interval_secs,
        "Starting external knowledge polling"
    );

    let feed_manager = Arc::new(RssFeedManager::new());
    let seen_ids: Arc<RwLock<std::collections::HashSet<String>>> =
        Arc::new(RwLock::new(std::collections::HashSet::new()));

    // Register all feeds
    for (i, feed_config) in config.rss_feeds.iter().enumerate() {
        let name = format!("feed_{}", i);
        if let Err(e) = feed_manager
            .add_feed(name.clone(), feed_config.clone())
            .await
        {
            error!(feed_url = %feed_config.url, error = %e, "Failed to register RSS feed");
        } else {
            info!(feed_url = %feed_config.url, name = %name, "Registered RSS feed");
        }
    }

    let mut ticker = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("External polling shutting down");
                break;
            }
            _ = ticker.tick() => {}
        }

        info!("Running external knowledge poll cycle");
        let results = feed_manager.fetch_all().await;
        let mut ingested_count = 0;

        for (feed_name, documents) in results {
            for doc in documents
                .into_iter()
                .take(config.max_items_per_cycle - ingested_count)
            {
                // Dedup: skip if we've seen this document ID
                {
                    let mut seen = seen_ids.write().await;
                    if seen.contains(&doc.id) {
                        continue;
                    }
                    seen.insert(doc.id.clone());

                    // Cap seen set at 50k entries
                    if seen.len() > 50_000 {
                        let to_remove: Vec<_> = seen.iter().take(10_000).cloned().collect();
                        for key in to_remove {
                            seen.remove(&key);
                        }
                    }
                }

                match ingest_external_document(&stores, &doc, &config.tenant_id, &feed_name).await {
                    Ok(_) => ingested_count += 1,
                    Err(e) => {
                        warn!(doc_id = %doc.id, error = %e, "Failed to ingest external document");
                    }
                }
            }

            if ingested_count >= config.max_items_per_cycle {
                break;
            }
        }

        if ingested_count > 0 {
            info!(count = ingested_count, "External poll cycle complete");
        }
    }
}

/// Convert an ExternalDocument to an EpistemicClaim and write to stores
async fn ingest_external_document(
    stores: &Stores,
    doc: &ExternalDocument,
    tenant_id: &str,
    feed_name: &str,
) -> cogkos_core::Result<uuid::Uuid> {
    let provenance = ProvenanceRecord::new(
        doc.id.clone(),
        format!("external/{}", doc.source),
        "rss_polling".to_string(),
    );

    let claimant = Claimant::ExternalPublic {
        source_name: doc.source.clone(),
    };

    let access_envelope = AccessEnvelope::from_claimant(tenant_id, &claimant);

    let mut claim = EpistemicClaim::new(
        doc.content.clone(),
        tenant_id,
        NodeType::Entity,
        claimant,
        access_envelope,
        provenance,
    );

    claim.knowledge_type = KnowledgeType::Experiential;
    claim.confidence = doc.confidence;

    if let Some(published) = doc.published_at {
        claim.t_known = published;
    }

    // Add metadata
    claim.metadata.insert(
        "external_source".to_string(),
        serde_json::Value::String(feed_name.to_string()),
    );
    claim.metadata.insert(
        "external_url".to_string(),
        serde_json::Value::String(doc.url.clone()),
    );
    claim.metadata.insert(
        "external_title".to_string(),
        serde_json::Value::String(doc.title.clone()),
    );

    let claim_id = claim.id;
    stores.claims.insert_claim(&claim).await?;

    Ok(claim_id)
}
