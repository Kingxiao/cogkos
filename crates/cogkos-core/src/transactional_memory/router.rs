//! Cross-instance router

use super::catalog::*;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Cross-instance router
pub struct CrossInstanceRouter {
    catalog: MetaKnowledgeCatalog,
    routing_cache: HashMap<String, RoutingCacheEntry>,
    cache_ttl_seconds: i64,
}

#[derive(Debug, Clone)]
struct RoutingCacheEntry {
    instances: Vec<String>,
    expires_at: DateTime<Utc>,
}

impl CrossInstanceRouter {
    pub fn new() -> Self {
        Self {
            catalog: MetaKnowledgeCatalog::new(),
            routing_cache: HashMap::new(),
            cache_ttl_seconds: 300, // 5 minutes
        }
    }

    pub fn with_cache_ttl(mut self, seconds: i64) -> Self {
        self.cache_ttl_seconds = seconds;
        self
    }

    /// Register an instance in the catalog
    pub fn register_instance(&mut self, entry: CatalogEntry) -> Result<()> {
        self.catalog.upsert(entry)?;
        Ok(())
    }

    /// Route query to appropriate instances
    pub fn route_query(&mut self, query: &str, domains: &[String]) -> RoutingDecision {
        let cache_key = format!("{}:{}", query, domains.join(","));

        // Check cache
        if let Some(cached) = self.routing_cache.get(&cache_key)
            && cached.expires_at > Utc::now()
        {
            return RoutingDecision {
                target_instances: cached.instances.clone(),
                routing_method: RoutingMethod::Cached,
                estimated_confidence: 0.8,
            };
        }

        // Find instances by domain
        let mut target_instances = HashMap::new();
        for domain in domains {
            for entry in self.catalog.get_by_domain(domain) {
                let score = entry.expertise_scores.get(domain).copied().unwrap_or(0.5);
                target_instances
                    .entry(entry.instance_id.clone())
                    .and_modify(|s: &mut f64| *s = s.max(score))
                    .or_insert(score);
            }
        }

        // Sort by expertise score
        let mut instances: Vec<(String, f64)> = target_instances.into_iter().collect();
        instances.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let instance_ids: Vec<String> = instances.into_iter().map(|(id, _)| id).collect();
        let is_empty = instance_ids.is_empty();

        // Cache result
        self.routing_cache.insert(
            cache_key,
            RoutingCacheEntry {
                instances: instance_ids.clone(),
                expires_at: Utc::now() + chrono::Duration::seconds(self.cache_ttl_seconds),
            },
        );

        RoutingDecision {
            target_instances: instance_ids,
            routing_method: RoutingMethod::DomainBased,
            estimated_confidence: if is_empty { 0.0 } else { 0.7 },
        }
    }

    /// Route by capability
    pub fn route_by_capability(&self, capability: InstanceCapability) -> RoutingDecision {
        let instances: Vec<String> = self
            .catalog
            .get_by_capability(capability)
            .iter()
            .map(|e| e.instance_id.clone())
            .collect();

        RoutingDecision {
            target_instances: instances.clone(),
            routing_method: RoutingMethod::CapabilityBased,
            estimated_confidence: if instances.is_empty() { 0.0 } else { 0.75 },
        }
    }

    /// Get routing path for knowledge transfer
    pub fn get_transfer_path(
        &self,
        source_instance: &str,
        target_domain: &str,
    ) -> Option<Vec<String>> {
        // Find shortest path from source to instance with target domain
        let target_instances: Vec<String> = self
            .catalog
            .get_by_domain(target_domain)
            .iter()
            .map(|e| e.instance_id.clone())
            .collect();

        if target_instances.is_empty() {
            return None;
        }

        // Simple direct routing for now
        // In production, would use graph search for optimal path
        target_instances
            .first()
            .map(|target| vec![source_instance.to_string(), target.clone()])
    }

    /// Clear expired cache entries
    pub fn clear_expired_cache(&mut self) {
        let now = Utc::now();
        self.routing_cache.retain(|_, entry| entry.expires_at > now);
    }

    /// Get catalog reference
    pub fn catalog(&self) -> &MetaKnowledgeCatalog {
        &self.catalog
    }

    /// Get mutable catalog reference
    pub fn catalog_mut(&mut self) -> &mut MetaKnowledgeCatalog {
        &mut self.catalog
    }
}

impl Default for CrossInstanceRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Routing decision result
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub target_instances: Vec<String>,
    pub routing_method: RoutingMethod,
    pub estimated_confidence: f64,
}

/// Routing method used
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingMethod {
    DomainBased,
    CapabilityBased,
    Cached,
    Optimized,
    Fallback,
}
