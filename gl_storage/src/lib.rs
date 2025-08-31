//! ABOUTME: Object storage abstraction for artifacts and captures
//! ABOUTME: Supports local filesystem and S3-compatible backends with retry logic

use std::io;
use std::path::PathBuf;

use backoff::{future::retry, ExponentialBackoff};
use bytes::Bytes;
use futures_util::TryStreamExt;
use object_store::{
    aws::AmazonS3Builder, local::LocalFileSystem, path::Path, ObjectStore, PutPayload,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use url::Url;

/// Storage errors
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Invalid URI: {0}")]
    InvalidUri(String),

    #[error("Object store error: {0}")]
    ObjectStore(#[from] object_store::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("MD5 checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Configuration error: {0}")]
    Config(String),
}

impl From<StorageError> for gl_core::Error {
    fn from(err: StorageError) -> Self {
        gl_core::Error::Storage(err.to_string())
    }
}

/// Storage URI that can be either local file or S3-compatible
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageUri {
    pub uri: String,
}

impl StorageUri {
    /// Create a new storage URI
    pub fn new(uri: impl Into<String>) -> std::result::Result<Self, StorageError> {
        let uri_str = uri.into();

        // Basic validation
        if uri_str.is_empty() {
            return Err(StorageError::InvalidUri("URI cannot be empty".to_string()));
        }

        // Parse to validate format
        if uri_str.starts_with("file://") {
            // File URI validation
            let path = uri_str.strip_prefix("file://").unwrap();
            if path.is_empty() {
                return Err(StorageError::InvalidUri(
                    "File path cannot be empty".to_string(),
                ));
            }
        } else if uri_str.starts_with("s3://") {
            // S3 URI validation
            let _url = Url::parse(&uri_str)?;
        } else {
            return Err(StorageError::InvalidUri(format!(
                "Unsupported URI scheme: {}",
                uri_str
            )));
        }

        Ok(StorageUri { uri: uri_str })
    }

    /// Get the scheme (file, s3, etc.)
    pub fn scheme(&self) -> &str {
        if let Some(pos) = self.uri.find("://") {
            &self.uri[..pos]
        } else {
            "unknown"
        }
    }

    /// Get the path component
    pub fn path(&self) -> std::result::Result<String, StorageError> {
        if self.uri.starts_with("file://") {
            Ok(self.uri.strip_prefix("file://").unwrap().to_string())
        } else if self.uri.starts_with("s3://") {
            let url = Url::parse(&self.uri)?;
            Ok(url.path().trim_start_matches('/').to_string())
        } else {
            Err(StorageError::InvalidUri(format!(
                "Cannot extract path from URI: {}",
                self.uri
            )))
        }
    }

    /// Get the bucket name for S3 URIs
    pub fn bucket(&self) -> std::result::Result<String, StorageError> {
        if self.uri.starts_with("s3://") {
            let url = Url::parse(&self.uri)?;
            url.host_str()
                .map(|h| h.to_string())
                .ok_or_else(|| StorageError::InvalidUri("No bucket in S3 URI".to_string()))
        } else {
            Err(StorageError::InvalidUri("Not an S3 URI".to_string()))
        }
    }
}

impl std::fmt::Display for StorageUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.uri)
    }
}

/// Result of a storage operation
#[derive(Debug)]
pub struct StorageResult {
    pub uri: StorageUri,
    pub size: usize,
    pub etag: Option<String>,
    pub checksum: Option<String>,
}

/// Storage configuration
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub base_dir: Option<PathBuf>,
    pub s3_region: Option<String>,
    pub s3_endpoint: Option<String>,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    pub retry_attempts: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            base_dir: None,
            s3_region: None,
            s3_endpoint: None,
            s3_access_key: None,
            s3_secret_key: None,
            retry_attempts: 3,
        }
    }
}

/// Storage trait for artifact operations
#[allow(async_fn_in_trait)]
pub trait Storage: Send + Sync {
    /// Store data at the given URI
    async fn put(
        &self,
        uri: &StorageUri,
        data: Bytes,
    ) -> std::result::Result<StorageResult, StorageError>;

    /// Retrieve data from the given URI
    async fn get(&self, uri: &StorageUri) -> std::result::Result<Bytes, StorageError>;

    /// Check if object exists
    async fn exists(&self, uri: &StorageUri) -> std::result::Result<bool, StorageError>;

    /// Delete object
    async fn delete(&self, uri: &StorageUri) -> std::result::Result<(), StorageError>;

    /// Get object metadata
    async fn metadata(&self, uri: &StorageUri) -> std::result::Result<StorageResult, StorageError>;
}

/// Multi-backend storage implementation
pub struct StorageManager {
    local: Option<Box<dyn ObjectStore>>,
    s3: Option<Box<dyn ObjectStore>>,
    #[allow(dead_code)]
    config: StorageConfig,
}

impl StorageManager {
    /// Create a new storage manager
    pub fn new(config: StorageConfig) -> std::result::Result<Self, StorageError> {
        let mut manager = StorageManager {
            local: None,
            s3: None,
            config: config.clone(),
        };

        // Initialize local filesystem storage
        if let Some(base_dir) = &config.base_dir {
            let local_fs =
                LocalFileSystem::new_with_prefix(base_dir).map_err(StorageError::ObjectStore)?;
            manager.local = Some(Box::new(local_fs));
            debug!("Initialized local filesystem storage at: {:?}", base_dir);
        }

        // Initialize S3 storage if credentials are provided
        if config.s3_access_key.is_some() && config.s3_secret_key.is_some() {
            let mut s3_builder = AmazonS3Builder::new();

            if let Some(region) = &config.s3_region {
                s3_builder = s3_builder.with_region(region);
            }

            if let Some(endpoint) = &config.s3_endpoint {
                s3_builder = s3_builder.with_endpoint(endpoint);
            }

            if let Some(access_key) = &config.s3_access_key {
                s3_builder = s3_builder.with_access_key_id(access_key);
            }

            if let Some(secret_key) = &config.s3_secret_key {
                s3_builder = s3_builder.with_secret_access_key(secret_key);
            }

            let s3_store = s3_builder.build().map_err(StorageError::ObjectStore)?;
            manager.s3 = Some(Box::new(s3_store));
            info!("Initialized S3 storage");
        }

        Ok(manager)
    }

    /// Get the appropriate object store for a URI
    fn get_store(&self, uri: &StorageUri) -> std::result::Result<&dyn ObjectStore, StorageError> {
        match uri.scheme() {
            "file" => self
                .local
                .as_ref()
                .map(|s| s.as_ref())
                .ok_or_else(|| StorageError::Config("Local storage not configured".to_string())),
            "s3" => self
                .s3
                .as_ref()
                .map(|s| s.as_ref())
                .ok_or_else(|| StorageError::Config("S3 storage not configured".to_string())),
            scheme => Err(StorageError::InvalidUri(format!(
                "Unsupported scheme: {}",
                scheme
            ))),
        }
    }

    /// Convert storage URI to object store path
    fn to_object_path(&self, uri: &StorageUri) -> std::result::Result<Path, StorageError> {
        let path_str = uri.path()?;
        Ok(Path::from(path_str))
    }

    /// Calculate MD5 checksum
    fn calculate_checksum(data: &[u8]) -> String {
        let digest = md5::compute(data);
        hex::encode(digest.0)
    }

    /// Retry wrapper for operations
    async fn with_retry<F, Fut, T>(&self, operation: F) -> std::result::Result<T, StorageError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, object_store::Error>>,
    {
        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(std::time::Duration::from_secs(60)),
            ..Default::default()
        };

        retry(backoff, || async {
            operation().await.map_err(|e| {
                warn!("Storage operation failed, will retry: {}", e);
                backoff::Error::transient(e)
            })
        })
        .await
        .map_err(StorageError::ObjectStore)
    }
}

impl Storage for StorageManager {
    async fn put(
        &self,
        uri: &StorageUri,
        data: Bytes,
    ) -> std::result::Result<StorageResult, StorageError> {
        let store = self.get_store(uri)?;
        let path = self.to_object_path(uri)?;
        let size = data.len();
        let checksum = Self::calculate_checksum(&data);

        debug!("Storing {} bytes at {}", size, uri);

        let payload = PutPayload::from(data);

        let result = self
            .with_retry(|| async { store.put(&path, payload.clone()).await })
            .await?;

        info!("Successfully stored {} bytes at {}", size, uri);

        Ok(StorageResult {
            uri: uri.clone(),
            size,
            etag: result.e_tag,
            checksum: Some(checksum),
        })
    }

    async fn get(&self, uri: &StorageUri) -> std::result::Result<Bytes, StorageError> {
        let store = self.get_store(uri)?;
        let path = self.to_object_path(uri)?;

        debug!("Retrieving data from {}", uri);

        let stream = self.with_retry(|| async { store.get(&path).await }).await?;

        let data = stream
            .into_stream()
            .try_collect::<Vec<_>>()
            .await
            .map_err(StorageError::ObjectStore)?
            .into_iter()
            .flatten()
            .collect::<Bytes>();

        debug!("Retrieved {} bytes from {}", data.len(), uri);
        Ok(data)
    }

    async fn exists(&self, uri: &StorageUri) -> std::result::Result<bool, StorageError> {
        let store = self.get_store(uri)?;
        let path = self.to_object_path(uri)?;

        match self.with_retry(|| async { store.head(&path).await }).await {
            Ok(_) => Ok(true),
            Err(StorageError::ObjectStore(object_store::Error::NotFound { .. })) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn delete(&self, uri: &StorageUri) -> std::result::Result<(), StorageError> {
        let store = self.get_store(uri)?;
        let path = self.to_object_path(uri)?;

        debug!("Deleting object at {}", uri);

        self.with_retry(|| async { store.delete(&path).await })
            .await?;

        info!("Successfully deleted object at {}", uri);
        Ok(())
    }

    async fn metadata(&self, uri: &StorageUri) -> std::result::Result<StorageResult, StorageError> {
        let store = self.get_store(uri)?;
        let path = self.to_object_path(uri)?;

        let metadata = self
            .with_retry(|| async { store.head(&path).await })
            .await?;

        Ok(StorageResult {
            uri: uri.clone(),
            size: metadata.size,
            etag: metadata.e_tag,
            checksum: None, // Would need to fetch data to calculate
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_uri_creation() {
        // Valid URIs
        let file_uri = StorageUri::new("file:///tmp/test.jpg").unwrap();
        assert_eq!(file_uri.scheme(), "file");
        assert_eq!(file_uri.path().unwrap(), "/tmp/test.jpg");

        let s3_uri = StorageUri::new("s3://mybucket/test.jpg").unwrap();
        assert_eq!(s3_uri.scheme(), "s3");
        assert_eq!(s3_uri.bucket().unwrap(), "mybucket");
        assert_eq!(s3_uri.path().unwrap(), "test.jpg");

        // Invalid URIs
        assert!(StorageUri::new("").is_err());
        assert!(StorageUri::new("invalid://test").is_err());
        assert!(StorageUri::new("file://").is_err());
    }

    #[test]
    fn test_storage_config_defaults() {
        let config = StorageConfig::default();
        assert!(config.base_dir.is_none());
        assert_eq!(config.retry_attempts, 3);
        assert!(config.s3_region.is_none());
    }

    #[test]
    fn test_checksum_calculation() {
        let data = b"Hello, world!";
        let checksum = StorageManager::calculate_checksum(data);
        assert_eq!(checksum, "6cd3556deb0da54bca060b4c39479839");
    }

    #[test]
    fn test_storage_uri_display() {
        let uri = StorageUri::new("s3://bucket/key").unwrap();
        assert_eq!(format!("{}", uri), "s3://bucket/key");
    }
}
