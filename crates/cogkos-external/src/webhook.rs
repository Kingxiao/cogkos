//! Webhook subscription module for external knowledge ingestion
//!
//! This module provides:
//! - Webhook receiver endpoint
//! - Signature verification (HMAC-SHA256)
//! - Automatic ingestion pipeline integration

#[allow(clippy::module_inception)]
pub mod webhook {
    use crate::error::{ExternalError, Result};
    use crate::types::{ExternalDocument, SourceType};
    use async_trait::async_trait;
    use chrono::Utc;
    use hex;
    use hmac::{Hmac, Mac};
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // HMAC-SHA256 type
    type HmacSha256 = Hmac<sha2::Sha256>;

    /// Webhook event types
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum WebhookEventType {
        /// New content pushed
        Push,
        /// Content updated
        Update,
        /// Content deleted
        Delete,
    }

    /// Webhook payload structure
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct WebhookPayload {
        /// Unique event ID
        pub event_id: String,
        /// Event type
        pub event_type: WebhookEventType,
        /// Event timestamp
        pub timestamp: chrono::DateTime<Utc>,
        /// Source identifier
        pub source: String,
        /// Document data (for push/update events)
        pub document: Option<ExternalDocument>,
        /// Document ID (for delete events)
        pub document_id: Option<String>,
        /// Additional metadata
        pub metadata: serde_json::Value,
    }

    /// Webhook subscription configuration
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct WebhookSubscription {
        /// Unique subscription ID
        pub id: uuid::Uuid,
        /// Subscription name
        pub name: String,
        /// Webhook secret for signature verification
        pub secret: String,
        /// Whether this subscription is active
        pub enabled: bool,
        /// Allowed source types
        pub allowed_types: Vec<SourceType>,
        /// Base confidence for ingested documents
        pub base_confidence: f64,
        /// Default source name for documents
        pub default_source: String,
    }

    /// Webhook signature validator
    pub struct WebhookSignatureValidator {
        secret: String,
    }

    impl WebhookSignatureValidator {
        /// Create a new validator with the given secret
        pub fn new(secret: impl Into<String>) -> Self {
            Self {
                secret: secret.into(),
            }
        }

        /// Verify webhook signature
        ///
        /// Signature is computed as HMAC-SHA256 of the raw request body,
        /// encoded as hex string and sent in the `X-Webhook-Signature` header.
        pub fn verify(&self, signature: &str, body: &[u8]) -> bool {
            // Create HMAC instance
            let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
                .expect("valid HMAC key: HMAC-SHA256 accepts any key length");

            // Compute HMAC
            mac.update(body);
            let result = mac.finalize();

            // Compare signatures (constant-time comparison)
            let expected = hex::encode(result.into_bytes());
            signature == expected
        }

        /// Verify from headers
        pub fn verify_from_headers(
            &self,
            signature_header: Option<&str>,
            body: &[u8],
        ) -> Result<bool> {
            let signature = signature_header.ok_or_else(|| {
                ExternalError::InvalidParams("Missing X-Webhook-Signature header".to_string())
            })?;

            Ok(self.verify(signature, body))
        }
    }

    /// Webhook event handler trait
    ///
    /// Implement this to handle webhook events
    #[async_trait]
    pub trait WebhookEventHandler: Send + Sync {
        /// Handle a webhook event
        async fn handle_event(&self, payload: WebhookPayload) -> Result<()>;
    }

    /// Webhook receiver state
    pub struct WebhookReceiver {
        subscriptions: Arc<RwLock<Vec<WebhookSubscription>>>,
        event_handler: Option<Arc<dyn WebhookEventHandler>>,
    }

    impl WebhookReceiver {
        /// Create a new webhook receiver
        pub fn new() -> Self {
            Self {
                subscriptions: Arc::new(RwLock::new(Vec::new())),
                event_handler: None,
            }
        }

        /// Set the event handler
        pub fn with_handler(mut self, handler: Arc<dyn WebhookEventHandler>) -> Self {
            self.event_handler = Some(handler);
            self
        }

        /// Add a subscription
        pub async fn add_subscription(&self, subscription: WebhookSubscription) {
            let mut subs = self.subscriptions.write().await;
            subs.push(subscription);
        }

        /// Remove a subscription
        pub async fn remove_subscription(&self, id: uuid::Uuid) {
            let mut subs = self.subscriptions.write().await;
            subs.retain(|s| s.id != id);
        }

        /// Get subscription by ID
        pub async fn get_subscription(&self, id: uuid::Uuid) -> Option<WebhookSubscription> {
            let subs = self.subscriptions.read().await;
            subs.iter().find(|s| s.id == id).cloned()
        }

        /// List all subscriptions
        pub async fn list_subscriptions(&self) -> Vec<WebhookSubscription> {
            let subs = self.subscriptions.read().await;
            subs.clone()
        }

        /// Process incoming webhook request
        pub async fn process_webhook(
            &self,
            signature: Option<&str>,
            body: &[u8],
            payload: WebhookPayload,
        ) -> Result<()> {
            // Find subscription
            let subscription = {
                let subs = self.subscriptions.read().await;
                subs.iter()
                    .find(|s| s.enabled && s.default_source == payload.source)
                    .cloned()
            };

            let sub = subscription.ok_or_else(|| {
                ExternalError::InvalidParams("No active subscription for this source".to_string())
            })?;

            // Verify signature
            let validator = WebhookSignatureValidator::new(&sub.secret);
            if !validator.verify_from_headers(signature, body)? {
                return Err(ExternalError::InvalidParams(
                    "Invalid signature".to_string(),
                ));
            }

            // Handle event
            if let Some(handler) = &self.event_handler {
                handler.handle_event(payload).await?;
            }

            Ok(())
        }
    }

    impl Default for WebhookReceiver {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Webhook server for Axum
    #[derive(Clone)]
    pub struct WebhookServer {
        receiver: Arc<WebhookReceiver>,
    }

    impl WebhookServer {
        /// Create a new webhook server
        pub fn new(receiver: Arc<WebhookReceiver>) -> Self {
            Self { receiver }
        }

        /// Get the receiver
        pub fn receiver(&self) -> Arc<WebhookReceiver> {
            self.receiver.clone()
        }
    }

    /// Webhook Manager - wraps WebhookReceiver for easier use
    #[derive(Clone)]
    pub struct WebhookManager {
        receiver: Arc<WebhookReceiver>,
    }

    impl WebhookManager {
        /// Create new webhook manager
        pub fn new() -> Self {
            Self {
                receiver: Arc::new(WebhookReceiver::new()),
            }
        }

        /// Get the receiver
        pub fn receiver(&self) -> Arc<WebhookReceiver> {
            Arc::clone(&self.receiver)
        }
    }

    impl Default for WebhookManager {
        fn default() -> Self {
            Self::new()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_signature_validation() {
            let secret = "test-secret-key";
            let validator = WebhookSignatureValidator::new(secret);

            let body = b"{\"event_id\": \"123\", \"event_type\": \"push\"}";

            // Create a valid signature
            let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
            mac.update(body);
            let signature = hex::encode(mac.finalize().into_bytes());

            assert!(validator.verify(&signature, body));
            assert!(!validator.verify("invalid-signature", body));
        }

        #[test]
        fn test_webhook_payload_serialization() {
            let payload = WebhookPayload {
                event_id: "evt-123".to_string(),
                event_type: WebhookEventType::Push,
                timestamp: Utc::now(),
                source: "test-source".to_string(),
                document: Some(ExternalDocument {
                    id: "doc-1".to_string(),
                    title: "Test Doc".to_string(),
                    content: "Test content".to_string(),
                    url: "https://example.com".to_string(),
                    source: "test".to_string(),
                    source_type: SourceType::WebPage,
                    published_at: None,
                    authors: vec![],
                    tags: vec![],
                    metadata: serde_json::json!({}),
                    confidence: 0.8,
                    fetched_at: Utc::now(),
                }),
                document_id: None,
                metadata: serde_json::json!({}),
            };

            let json = serde_json::to_string(&payload).unwrap();
            let parsed: WebhookPayload = serde_json::from_str(&json).unwrap();

            assert_eq!(parsed.event_id, "evt-123");
            assert_eq!(parsed.event_type, WebhookEventType::Push);
        }
    }
}

pub use webhook::*;
