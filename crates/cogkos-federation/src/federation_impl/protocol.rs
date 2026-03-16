use super::anonymizer::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use cogkos_core::EpistemicClaim;

/// Federation protocol trait
#[async_trait]
pub trait FederationProtocol {
    /// Export insights to file
    async fn export_to_file(&self, insights: &[AnonymousInsight], path: &Path) -> Result<()>;

    /// Import insights from file
    async fn import_from_file(&self, path: &Path) -> Result<Vec<AnonymousInsight>>;

    /// Export insights via API
    async fn export_via_api(
        &self,
        insights: &[AnonymousInsight],
        endpoint: &str,
        auth_token: &str,
    ) -> Result<()>;

    /// Import insights via API
    async fn import_via_api(
        &self,
        endpoint: &str,
        auth_token: &str,
    ) -> Result<Vec<AnonymousInsight>>;
}

/// Cross-instance authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossInstanceAuth {
    pub instance_id: String,
    pub public_key: String,
    pub auth_token: String,
    pub expires_at: DateTime<Utc>,
    pub permissions: Vec<FederationPermission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FederationPermission {
    Export,
    Import,
    Query,
    Admin,
}

/// Cross-instance authenticator
pub struct CrossInstanceAuthenticator {
    trusted_instances: HashMap<String, CrossInstanceAuth>,
    token_cache: HashMap<String, (String, DateTime<Utc>)>, // token -> (instance_id, expires)
}

impl CrossInstanceAuthenticator {
    pub fn new() -> Self {
        Self {
            trusted_instances: HashMap::new(),
            token_cache: HashMap::new(),
        }
    }

    /// Register a trusted instance
    pub fn register_instance(&mut self, auth: CrossInstanceAuth) {
        self.trusted_instances
            .insert(auth.instance_id.clone(), auth);
    }

    /// Authenticate incoming request
    pub fn authenticate(&mut self, auth_token: &str) -> Result<CrossInstanceAuth> {
        // Check cache first
        if let Some((instance_id, expires)) = self.token_cache.get(auth_token)
            && *expires > Utc::now()
        {
            return self
                .trusted_instances
                .get(instance_id)
                .cloned()
                .ok_or_else(|| {
                    FederationProtocolError::AuthenticationFailed("Instance not found".to_string())
                });
        }

        // Find matching token
        for (instance_id, auth) in &self.trusted_instances {
            if auth.auth_token == auth_token && auth.expires_at > Utc::now() {
                // Cache and return
                self.token_cache.insert(
                    auth_token.to_string(),
                    (instance_id.clone(), auth.expires_at),
                );
                return Ok(auth.clone());
            }
        }

        Err(FederationProtocolError::AuthenticationFailed(
            "Invalid or expired token".to_string(),
        ))
    }

    /// Check if instance has permission
    pub fn check_permission(
        &self,
        instance_id: &str,
        permission: FederationPermission,
    ) -> Result<()> {
        let auth = self.trusted_instances.get(instance_id).ok_or_else(|| {
            FederationProtocolError::AuthenticationFailed("Instance not registered".to_string())
        })?;

        if auth.expires_at < Utc::now() {
            return Err(FederationProtocolError::AuthenticationFailed(
                "Auth expired".to_string(),
            ));
        }

        if !auth.permissions.contains(&permission) {
            return Err(FederationProtocolError::AuthenticationFailed(
                "Permission denied".to_string(),
            ));
        }

        Ok(())
    }

    /// Revoke instance access
    pub fn revoke_instance(&mut self, instance_id: &str) -> bool {
        self.trusted_instances.remove(instance_id).is_some()
    }

    /// Clean expired tokens
    pub fn cleanup_expired(&mut self) {
        let now = Utc::now();
        self.trusted_instances
            .retain(|_, auth| auth.expires_at > now);
        self.token_cache.retain(|_, (_, expires)| *expires > now);
    }
}

impl Default for CrossInstanceAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP-based federation protocol implementation
pub struct HttpFederationProtocol {
    client: reqwest::Client,
    authenticator: CrossInstanceAuthenticator,
}

impl HttpFederationProtocol {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            authenticator: CrossInstanceAuthenticator::new(),
        }
    }

    /// Export insights as JSON to file
    pub fn export_to_file_sync(&self, insights: &[AnonymousInsight], path: &Path) -> Result<()> {
        let export = FederationExport {
            version: "1.0".to_string(),
            exported_at: Utc::now(),
            insight_count: insights.len(),
            insights: insights.to_vec(),
        };

        let json = serde_json::to_string_pretty(&export)?;
        fs::write(path, json)?;

        Ok(())
    }

    /// Import insights from JSON file
    pub fn import_from_file_sync(&self, path: &Path) -> Result<Vec<AnonymousInsight>> {
        let json = fs::read_to_string(path)?;
        let export: FederationExport = serde_json::from_str(&json)?;

        // Validate version
        if export.version != "1.0" {
            return Err(FederationProtocolError::ProtocolError(format!(
                "Unsupported export version: {}",
                export.version
            )));
        }

        Ok(export.insights)
    }

    /// Get authenticator reference
    pub fn authenticator(&self) -> &CrossInstanceAuthenticator {
        &self.authenticator
    }

    /// Get mutable authenticator reference
    pub fn authenticator_mut(&mut self) -> &mut CrossInstanceAuthenticator {
        &mut self.authenticator
    }
}

impl Default for HttpFederationProtocol {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FederationProtocol for HttpFederationProtocol {
    async fn export_to_file(&self, insights: &[AnonymousInsight], path: &Path) -> Result<()> {
        self.export_to_file_sync(insights, path)
    }

    async fn import_from_file(&self, path: &Path) -> Result<Vec<AnonymousInsight>> {
        self.import_from_file_sync(path)
    }

    async fn export_via_api(
        &self,
        insights: &[AnonymousInsight],
        endpoint: &str,
        auth_token: &str,
    ) -> Result<()> {
        let export = FederationExport {
            version: "1.0".to_string(),
            exported_at: Utc::now(),
            insight_count: insights.len(),
            insights: insights.to_vec(),
        };

        let response = self
            .client
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", auth_token))
            .header("Content-Type", "application/json")
            .json(&export)
            .send()
            .await
            .map_err(|e| FederationProtocolError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(FederationProtocolError::HttpError(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        Ok(())
    }

    async fn import_via_api(
        &self,
        endpoint: &str,
        auth_token: &str,
    ) -> Result<Vec<AnonymousInsight>> {
        let response = self
            .client
            .get(endpoint)
            .header("Authorization", format!("Bearer {}", auth_token))
            .send()
            .await
            .map_err(|e| FederationProtocolError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(FederationProtocolError::HttpError(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let export: FederationExport = response
            .json()
            .await
            .map_err(|e| FederationProtocolError::HttpError(e.to_string()))?;

        Ok(export.insights)
    }
}

/// Federation export format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationExport {
    pub version: String,
    pub exported_at: DateTime<Utc>,
    pub insight_count: usize,
    pub insights: Vec<AnonymousInsight>,
}

/// Federation manager - high-level interface
pub struct FederationManager {
    anonymizer: InsightAnonymizer,
    protocol: HttpFederationProtocol,
    instance_id: String,
}

impl FederationManager {
    pub fn new(instance_id: impl Into<String>) -> Self {
        Self {
            anonymizer: InsightAnonymizer::new(AnonymizationConfig::default()),
            protocol: HttpFederationProtocol::new(),
            instance_id: instance_id.into(),
        }
    }

    pub fn with_config(mut self, config: AnonymizationConfig) -> Self {
        self.anonymizer = InsightAnonymizer::new(config);
        self
    }

    /// Export claims to anonymous insights
    pub fn export_insights(&self, claims: &[EpistemicClaim]) -> Vec<AnonymousInsight> {
        self.anonymizer.anonymize_batch(claims, &self.instance_id)
    }

    /// Export insights to file
    pub fn export_to_file(&self, insights: &[AnonymousInsight], path: &Path) -> Result<()> {
        self.protocol.export_to_file_sync(insights, path)
    }

    /// Import insights from file
    pub fn import_from_file(&self, path: &Path) -> Result<Vec<AnonymousInsight>> {
        self.protocol.import_from_file_sync(path)
    }

    /// Register trusted instance
    pub fn register_trusted_instance(&mut self, auth: CrossInstanceAuth) {
        self.protocol.authenticator_mut().register_instance(auth);
    }

    /// Authenticate request
    pub fn authenticate(&mut self, token: &str) -> Result<CrossInstanceAuth> {
        self.protocol.authenticator_mut().authenticate(token)
    }

    /// Get protocol reference for API operations
    pub fn protocol(&self) -> &HttpFederationProtocol {
        &self.protocol
    }
}

/// Validate imported insights
pub fn validate_imported_insights(insights: &[AnonymousInsight]) -> ValidationResult {
    let mut valid = 0;
    let mut invalid = 0;
    let mut errors = Vec::new();

    for (i, insight) in insights.iter().enumerate() {
        // Check required fields
        if insight.anonymized_content.is_empty() {
            invalid += 1;
            errors.push(format!("Insight {}: Empty content", i));
            continue;
        }

        if insight.confidence < 0.0 || insight.confidence > 1.0 {
            invalid += 1;
            errors.push(format!(
                "Insight {}: Invalid confidence {}",
                i, insight.confidence
            ));
            continue;
        }

        // Check for duplicates based on content hash
        // In production, would check against existing knowledge

        valid += 1;
    }

    ValidationResult {
        total: insights.len(),
        valid,
        invalid,
        errors,
    }
}

/// Validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.invalid == 0
    }

    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.valid as f64 / self.total as f64
        }
    }
}
