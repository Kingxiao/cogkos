//! S3 object store implementation using AWS SDK

use async_trait::async_trait;
use aws_config::Region;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::{Client, primitives::ByteStream};
use cogkos_core::{CogKosError, Result};
use std::sync::Arc;
use tokio::fs;

/// S3 object store using AWS SDK
pub struct S3Store {
    client: Client,
    bucket: String,
    region: String,
}

/// Local filesystem object store (fallback for development)
pub struct LocalStore {
    base_path: std::path::PathBuf,
    bucket: String,
}

impl LocalStore {
    /// Create new local store
    pub async fn new(bucket: &str) -> Result<Self> {
        let base_path = std::path::PathBuf::from("./data/objects").join(bucket);
        fs::create_dir_all(&base_path)
            .await
            .map_err(|e| CogKosError::Storage(format!("Failed to create directory: {}", e)))?;

        Ok(Self {
            base_path,
            bucket: bucket.to_string(),
        })
    }

    /// Build a safe file path from the given key, preventing path traversal attacks.
    fn build_path(&self, key: &str) -> Result<std::path::PathBuf> {
        // Reject keys with path traversal components
        if key.contains("..") || key.starts_with('/') || key.starts_with('\\') || key.contains('\0')
        {
            return Err(CogKosError::InvalidInput(format!(
                "Invalid object key (path traversal detected): {}",
                key
            )));
        }
        let path = self.base_path.join(key);
        // Double-check: resolved path must still be under base_path
        // Use lexical comparison since files may not exist yet (canonicalize needs existing paths)
        let base_str = self.base_path.to_string_lossy();
        let path_str = path.to_string_lossy();
        if !path_str.starts_with(base_str.as_ref()) {
            return Err(CogKosError::InvalidInput(format!(
                "Object key resolves outside storage directory: {}",
                key
            )));
        }
        Ok(path)
    }
}

#[async_trait]
impl super::ObjectStore for LocalStore {
    async fn upload(&self, key: &str, data: &[u8], _content_type: &str) -> Result<String> {
        let path = self.build_path(key)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| CogKosError::Storage(format!("Failed to create directory: {}", e)))?;
        }

        fs::write(&path, data)
            .await
            .map_err(|e| CogKosError::Storage(format!("Failed to write file: {}", e)))?;

        Ok(format!("{}/{}", self.bucket, key))
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.build_path(key)?;
        fs::read(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CogKosError::NotFound(format!("Object not found: {}", key))
            } else {
                CogKosError::Storage(format!("Failed to read file: {}", e))
            }
        })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.build_path(key)?;
        fs::remove_file(&path)
            .await
            .map_err(|e| CogKosError::Storage(format!("Failed to delete file: {}", e)))?;

        Ok(())
    }

    async fn presigned_url(&self, key: &str, _expiry_secs: u64) -> Result<String> {
        let path = self.build_path(key)?;
        Ok(format!("file://{}", path.display()))
    }
}

impl S3Store {
    /// Create new S3 store with AWS credentials
    pub async fn new(
        endpoint: Option<&str>,
        region: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<Self> {
        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(Region::new(region.to_string()))
            .credentials_provider(Credentials::new(
                access_key, secret_key, None, None, "cogkos",
            ));

        if let Some(endpoint_url) = endpoint {
            // For SeaweedFS, DigitalOcean Spaces, etc.
            config_loader = config_loader.endpoint_url(endpoint_url);
        }

        let config = config_loader.load().await;
        let s3_config = aws_sdk_s3::Config::from(&config);
        let client = Client::from_conf(s3_config);

        Ok(Self {
            client,
            bucket: bucket.to_string(),
            region: region.to_string(),
        })
    }

    /// Create new S3 store using environment variables or IAM role
    pub async fn from_env(region: &str, bucket: &str) -> Result<Self> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(Region::new(region.to_string()))
            .load()
            .await;

        let s3_config = aws_sdk_s3::Config::from(&config);
        let client = Client::from_conf(s3_config);

        Ok(Self {
            client,
            bucket: bucket.to_string(),
            region: region.to_string(),
        })
    }

    /// Get bucket name
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Get region
    pub fn region(&self) -> &str {
        &self.region
    }
}

#[async_trait]
impl super::ObjectStore for S3Store {
    /// Upload object to S3
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<String> {
        let byte_stream = ByteStream::from(data.to_vec());

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(byte_stream)
            .send()
            .await
            .map_err(|e| CogKosError::Storage(format!("S3 upload failed: {}", e)))?;

        // Return the S3 URL
        Ok(format!("s3://{}/{}", self.bucket, key))
    }

    /// Download object from S3
    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        let result = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                if e.to_string().contains("NoSuchKey") {
                    CogKosError::NotFound(format!("Object not found: {}", key))
                } else {
                    CogKosError::Storage(format!("S3 download failed: {}", e))
                }
            })?;

        let bytes = result
            .body
            .collect()
            .await
            .map_err(|e| CogKosError::Storage(format!("Failed to read S3 response: {}", e)))?
            .to_vec();

        Ok(bytes)
    }

    /// Delete object from S3
    async fn delete(&self, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| CogKosError::Storage(format!("S3 delete failed: {}", e)))?;

        Ok(())
    }

    /// Generate presigned URL for object access
    async fn presigned_url(&self, key: &str, expiry_secs: u64) -> Result<String> {
        // Use the presigned request feature
        let presigning_config = PresigningConfig::expires_in(std::time::Duration::from_secs(
            expiry_secs,
        ))
        .map_err(|e| CogKosError::Storage(format!("Failed to create presigning config: {}", e)))?;

        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning_config)
            .await
            .map_err(|e| CogKosError::Storage(format!("Failed to create presigned URL: {}", e)))?;

        Ok(presigned.uri().to_string())
    }
}

/// S3 Store with automatic fallback to local storage
pub enum S3StoreWithFallback {
    S3(Arc<S3Store>),
    Local(Arc<LocalStore>),
}

impl S3StoreWithFallback {
    /// Create store with automatic fallback
    /// If S3 credentials are provided, use S3. Otherwise, fallback to local storage.
    pub async fn new(
        endpoint: Option<&str>,
        region: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<Self> {
        // Check if we have valid credentials
        if access_key.is_empty() || secret_key.is_empty() {
            tracing::info!("No S3 credentials provided, using local storage");
            let local = LocalStore::new(bucket).await?;
            return Ok(Self::Local(Arc::new(local)));
        }

        // Try to create S3 store
        match S3Store::new(endpoint, region, bucket, access_key, secret_key).await {
            Ok(store) => {
                tracing::info!("Successfully connected to S3 bucket: {}", bucket);
                Ok(Self::S3(Arc::new(store)))
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to connect to S3, falling back to local storage: {}",
                    e
                );
                let local = LocalStore::new(bucket).await?;
                Ok(Self::Local(Arc::new(local)))
            }
        }
    }

    /// Create from environment variables
    pub async fn from_env(bucket: &str) -> Result<Self> {
        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        let access_key = std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default();
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default();
        let endpoint = std::env::var("S3_ENDPOINT").ok();

        Self::new(
            endpoint.as_deref(),
            &region,
            bucket,
            &access_key,
            &secret_key,
        )
        .await
    }
}

#[async_trait]
impl super::ObjectStore for S3StoreWithFallback {
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<String> {
        match self {
            Self::S3(store) => store.upload(key, data, content_type).await,
            Self::Local(store) => store.upload(key, data, content_type).await,
        }
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        match self {
            Self::S3(store) => store.download(key).await,
            Self::Local(store) => store.download(key).await,
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        match self {
            Self::S3(store) => store.delete(key).await,
            Self::Local(store) => store.delete(key).await,
        }
    }

    async fn presigned_url(&self, key: &str, expiry_secs: u64) -> Result<String> {
        match self {
            Self::S3(store) => store.presigned_url(key, expiry_secs).await,
            Self::Local(store) => store.presigned_url(key, expiry_secs).await,
        }
    }
}

/// In-memory object store for testing
pub struct InMemoryObjectStore {
    objects: std::sync::RwLock<std::collections::HashMap<String, (Vec<u8>, String)>>,
}

impl InMemoryObjectStore {
    pub fn new() -> Self {
        Self {
            objects: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for InMemoryObjectStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl super::ObjectStore for InMemoryObjectStore {
    async fn upload(&self, key: &str, data: &[u8], content_type: &str) -> Result<String> {
        let mut objects = self
            .objects
            .write()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        objects.insert(key.to_string(), (data.to_vec(), content_type.to_string()));
        Ok(key.to_string())
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        let objects = self
            .objects
            .read()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        objects
            .get(key)
            .map(|(data, _)| data.clone())
            .ok_or_else(|| CogKosError::NotFound(format!("Object not found: {}", key)))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut objects = self
            .objects
            .write()
            .map_err(|_| CogKosError::Internal("Lock poisoned".to_string()))?;
        objects.remove(key);
        Ok(())
    }

    async fn presigned_url(&self, key: &str, _expiry_secs: u64) -> Result<String> {
        Ok(format!("memory://{}", key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectStore;

    // --- LocalStore path traversal tests ---

    #[tokio::test]
    async fn test_local_store_rejects_dotdot() {
        let store = LocalStore::new("cogkos-test-store").await.unwrap();
        let result = store
            .upload("../../etc/passwd", b"evil", "text/plain")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[tokio::test]
    async fn test_local_store_rejects_absolute_path() {
        let store = LocalStore::new("cogkos-test-store").await.unwrap();
        let result = store.upload("/etc/passwd", b"evil", "text/plain").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_local_store_rejects_null_bytes() {
        let store = LocalStore::new("cogkos-test-store").await.unwrap();
        let result = store.upload("file\0.txt", b"evil", "text/plain").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_local_store_accepts_valid_key() {
        let store = LocalStore::new("cogkos-test-store").await.unwrap();
        let result = store
            .upload("tenant-1/docs/file.pdf", b"data", "application/pdf")
            .await;
        assert!(result.is_ok());
        // Cleanup
        let _ = store.delete("tenant-1/docs/file.pdf").await;
    }

    #[tokio::test]
    async fn test_local_store_download_missing_key() {
        let store = LocalStore::new("cogkos-test-store").await.unwrap();
        let result = store.download("nonexistent-key").await;
        assert!(result.is_err());
    }

    // --- InMemoryObjectStore tests ---

    #[tokio::test]
    async fn test_inmemory_store_roundtrip() {
        let store = InMemoryObjectStore::new();
        store
            .upload("key1", b"hello world", "text/plain")
            .await
            .unwrap();
        let data = store.download("key1").await.unwrap();
        assert_eq!(data, b"hello world");
    }

    #[tokio::test]
    async fn test_inmemory_store_delete() {
        let store = InMemoryObjectStore::new();
        store.upload("key1", b"data", "text/plain").await.unwrap();
        store.delete("key1").await.unwrap();
        assert!(store.download("key1").await.is_err());
    }

    #[tokio::test]
    async fn test_inmemory_store_download_missing() {
        let store = InMemoryObjectStore::new();
        let result = store.download("missing").await;
        assert!(result.is_err());
    }
}
