//! Transactional Memory - Meta-knowledge catalog

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Transactional memory errors
#[derive(Error, Debug)]
pub enum TransactionalMemoryError {
    #[error("Instance not found: {0}")]
    InstanceNotFound(String),
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),
    #[error("Routing error: {0}")]
    RoutingError(String),
    #[error("Catalog error: {0}")]
    CatalogError(String),
    #[error("Timeout")]
    Timeout,
    #[error("Conflict: {0}")]
    Conflict(String),
}

pub type Result<T> = std::result::Result<T, TransactionalMemoryError>;

/// Meta-knowledge catalog entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub instance_id: String,
    pub domain_tags: Vec<String>,
    pub expertise_scores: HashMap<String, f64>,
    pub knowledge_count: usize,
    pub last_updated: DateTime<Utc>,
    pub api_endpoint: String,
    pub auth_method: AuthMethod,
    pub capabilities: Vec<InstanceCapability>,
}

/// Authentication method for instance communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthMethod {
    None,
    ApiKey { key_hash: String },
    MutualTls { cert_fingerprint: String },
    Jwt { issuer: String },
}

/// Instance capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstanceCapability {
    KnowledgeRead,
    KnowledgeWrite,
    QueryProcessing,
    FederatedLearning,
    ModelServing,
    Analytics,
}

/// Meta-knowledge catalog - CRUD operations
pub struct MetaKnowledgeCatalog {
    entries: HashMap<String, CatalogEntry>,
    domain_index: HashMap<String, Vec<String>>, // domain -> instance_ids
    capability_index: HashMap<InstanceCapability, Vec<String>>,
}

impl MetaKnowledgeCatalog {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            domain_index: HashMap::new(),
            capability_index: HashMap::new(),
        }
    }

    /// Create or update catalog entry
    pub fn upsert(&mut self, entry: CatalogEntry) -> Result<()> {
        // Remove from old indices
        if let Some(old_entry) = self.entries.get(&entry.instance_id) {
            for domain in &old_entry.domain_tags {
                if let Some(instances) = self.domain_index.get_mut(domain) {
                    instances.retain(|id| id != &entry.instance_id);
                }
            }
            for capability in &old_entry.capabilities {
                if let Some(instances) = self.capability_index.get_mut(capability) {
                    instances.retain(|id| id != &entry.instance_id);
                }
            }
        }

        // Add to new indices
        for domain in &entry.domain_tags {
            self.domain_index
                .entry(domain.clone())
                .or_default()
                .push(entry.instance_id.clone());
        }
        for capability in &entry.capabilities {
            self.capability_index
                .entry(*capability)
                .or_default()
                .push(entry.instance_id.clone());
        }

        self.entries.insert(entry.instance_id.clone(), entry);
        Ok(())
    }

    /// Read catalog entry
    pub fn get(&self, instance_id: &str) -> Option<&CatalogEntry> {
        self.entries.get(instance_id)
    }

    /// Read by domain
    pub fn get_by_domain(&self, domain: &str) -> Vec<&CatalogEntry> {
        self.domain_index
            .get(domain)
            .map(|ids| ids.iter().filter_map(|id| self.entries.get(id)).collect())
            .unwrap_or_default()
    }

    /// Read by capability
    pub fn get_by_capability(&self, capability: InstanceCapability) -> Vec<&CatalogEntry> {
        self.capability_index
            .get(&capability)
            .map(|ids| ids.iter().filter_map(|id| self.entries.get(id)).collect())
            .unwrap_or_default()
    }

    /// Update catalog entry
    pub fn update(&mut self, instance_id: &str, update: CatalogUpdate) -> Result<()> {
        let entry = self
            .entries
            .get_mut(instance_id)
            .ok_or_else(|| TransactionalMemoryError::InstanceNotFound(instance_id.to_string()))?;

        if let Some(domains) = update.domain_tags {
            // Rebuild domain index
            for domain in &entry.domain_tags {
                if let Some(instances) = self.domain_index.get_mut(domain) {
                    instances.retain(|id| id != instance_id);
                }
            }
            for domain in &domains {
                self.domain_index
                    .entry(domain.clone())
                    .or_default()
                    .push(instance_id.to_string());
            }
            entry.domain_tags = domains;
        }

        if let Some(expertise) = update.expertise_scores {
            entry.expertise_scores = expertise;
        }

        if let Some(count) = update.knowledge_count {
            entry.knowledge_count = count;
        }

        if let Some(endpoint) = update.api_endpoint {
            entry.api_endpoint = endpoint;
        }

        if let Some(auth) = update.auth_method {
            entry.auth_method = auth;
        }

        if let Some(caps) = update.capabilities {
            // Rebuild capability index
            for cap in &entry.capabilities {
                if let Some(instances) = self.capability_index.get_mut(cap) {
                    instances.retain(|id| id != instance_id);
                }
            }
            for cap in &caps {
                self.capability_index
                    .entry(*cap)
                    .or_default()
                    .push(instance_id.to_string());
            }
            entry.capabilities = caps;
        }

        entry.last_updated = Utc::now();
        Ok(())
    }

    /// Delete catalog entry
    pub fn delete(&mut self, instance_id: &str) -> Result<bool> {
        let entry = self
            .entries
            .remove(instance_id)
            .ok_or_else(|| TransactionalMemoryError::InstanceNotFound(instance_id.to_string()))?;

        // Remove from indices
        for domain in &entry.domain_tags {
            if let Some(instances) = self.domain_index.get_mut(domain) {
                instances.retain(|id| id != instance_id);
            }
        }
        for capability in &entry.capabilities {
            if let Some(instances) = self.capability_index.get_mut(capability) {
                instances.retain(|id| id != instance_id);
            }
        }

        Ok(true)
    }

    /// List all entries
    pub fn list_all(&self) -> Vec<&CatalogEntry> {
        self.entries.values().collect()
    }

    /// Get domain coverage statistics
    pub fn domain_statistics(&self) -> HashMap<String, usize> {
        self.domain_index
            .iter()
            .map(|(domain, instances)| (domain.clone(), instances.len()))
            .collect()
    }
}

impl Default for MetaKnowledgeCatalog {
    fn default() -> Self {
        Self::new()
    }
}

/// Update payload for catalog entry
#[derive(Debug, Clone, Default)]
pub struct CatalogUpdate {
    pub domain_tags: Option<Vec<String>>,
    pub expertise_scores: Option<HashMap<String, f64>>,
    pub knowledge_count: Option<usize>,
    pub api_endpoint: Option<String>,
    pub auth_method: Option<AuthMethod>,
    pub capabilities: Option<Vec<InstanceCapability>>,
}
