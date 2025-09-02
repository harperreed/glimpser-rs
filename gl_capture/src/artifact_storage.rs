//! ABOUTME: Artifact storage service for capture snapshots and recordings
//! ABOUTME: Integrates gl_storage with capture system for persistent artifact storage

use bytes::Bytes;
use gl_core::{Error, Result};
use gl_storage::{Storage, StorageUri};
use std::time::SystemTime;
use tracing::{debug, info, instrument};

/// Configuration for artifact storage
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactStorageConfig {
    /// Base storage URI for artifacts (e.g., "s3://bucket/artifacts" or "file:///var/data/artifacts")
    pub base_uri: String,
    /// File extension for snapshot artifacts
    pub snapshot_extension: String,
    /// Include timestamp in artifact names
    pub include_timestamp: bool,
}

impl Default for ArtifactStorageConfig {
    fn default() -> Self {
        Self {
            base_uri: "file:///tmp/glimpser/artifacts".to_string(),
            snapshot_extension: "jpg".to_string(),
            include_timestamp: true,
        }
    }
}

/// Result of storing an artifact
#[derive(Debug, Clone)]
pub struct StoredArtifact {
    /// Storage URI where the artifact was saved
    pub uri: StorageUri,
    /// Size of the stored artifact in bytes
    pub size: usize,
    /// Content type/MIME type
    pub content_type: String,
    /// ETag from storage (if available)
    pub etag: Option<String>,
    /// MD5 checksum
    pub checksum: Option<String>,
}

/// Service for storing capture artifacts
pub struct ArtifactStorageService<S> {
    storage: S,
    config: ArtifactStorageConfig,
}

impl<S> ArtifactStorageService<S>
where
    S: Storage,
{
    /// Create a new artifact storage service
    pub fn new(storage: S, config: ArtifactStorageConfig) -> Self {
        Self { storage, config }
    }

    /// Store a snapshot artifact and return the storage URI
    #[instrument(skip(self, data), fields(size = data.len()))]
    pub async fn store_snapshot(&self, capture_id: &str, data: Bytes) -> Result<StoredArtifact> {
        let filename = self.generate_snapshot_filename(capture_id);
        let uri = self.build_artifact_uri(&filename)?;

        debug!("Storing snapshot artifact at {}", uri);

        let storage_result = self
            .storage
            .put(&uri, data)
            .await
            .map_err(|e| Error::Storage(e.to_string()))?;

        info!(
            capture_id = capture_id,
            uri = %uri,
            size = storage_result.size,
            "Successfully stored snapshot artifact"
        );

        Ok(StoredArtifact {
            uri,
            size: storage_result.size,
            content_type: "image/jpeg".to_string(),
            etag: storage_result.etag,
            checksum: storage_result.checksum,
        })
    }

    /// Store a recording artifact and return the storage URI
    #[instrument(skip(self, data), fields(size = data.len()))]
    pub async fn store_recording(
        &self,
        capture_id: &str,
        data: Bytes,
        format: &str,
    ) -> Result<StoredArtifact> {
        let filename = self.generate_recording_filename(capture_id, format);
        let uri = self.build_artifact_uri(&filename)?;

        debug!("Storing recording artifact at {}", uri);

        let storage_result = self
            .storage
            .put(&uri, data)
            .await
            .map_err(|e| Error::Storage(e.to_string()))?;

        let content_type = match format.to_lowercase().as_str() {
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            "avi" => "video/x-msvideo",
            _ => "application/octet-stream",
        };

        info!(
            capture_id = capture_id,
            uri = %uri,
            size = storage_result.size,
            format = format,
            "Successfully stored recording artifact"
        );

        Ok(StoredArtifact {
            uri,
            size: storage_result.size,
            content_type: content_type.to_string(),
            etag: storage_result.etag,
            checksum: storage_result.checksum,
        })
    }

    /// Retrieve an artifact by its storage URI
    #[instrument(skip(self))]
    pub async fn get_artifact(&self, uri: &StorageUri) -> Result<Bytes> {
        debug!("Retrieving artifact from {}", uri);

        self.storage
            .get(uri)
            .await
            .map_err(|e| Error::Storage(e.to_string()))
    }

    /// Check if an artifact exists
    pub async fn artifact_exists(&self, uri: &StorageUri) -> Result<bool> {
        self.storage
            .exists(uri)
            .await
            .map_err(|e| Error::Storage(e.to_string()))
    }

    /// Delete an artifact
    #[instrument(skip(self))]
    pub async fn delete_artifact(&self, uri: &StorageUri) -> Result<()> {
        debug!("Deleting artifact at {}", uri);

        self.storage
            .delete(uri)
            .await
            .map_err(|e| Error::Storage(e.to_string()))?;

        info!("Successfully deleted artifact at {}", uri);
        Ok(())
    }

    /// Generate a filename for a snapshot
    fn generate_snapshot_filename(&self, capture_id: &str) -> String {
        if self.config.include_timestamp {
            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            format!(
                "snapshot_{}_{}.{}",
                capture_id, timestamp, self.config.snapshot_extension
            )
        } else {
            format!("snapshot_{}.{}", capture_id, self.config.snapshot_extension)
        }
    }

    /// Generate a filename for a recording
    fn generate_recording_filename(&self, capture_id: &str, format: &str) -> String {
        if self.config.include_timestamp {
            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            format!("recording_{}_{}.{}", capture_id, timestamp, format)
        } else {
            format!("recording_{}.{}", capture_id, format)
        }
    }

    /// Build a full artifact URI from a filename
    fn build_artifact_uri(&self, filename: &str) -> Result<StorageUri> {
        let full_path = if self.config.base_uri.ends_with('/') {
            format!("{}{}", self.config.base_uri, filename)
        } else {
            format!("{}/{}", self.config.base_uri, filename)
        };

        StorageUri::new(full_path).map_err(|e| Error::Storage(e.to_string()))
    }
}

/// Helper function to take a snapshot from a capture source and store it
pub async fn snapshot_and_store<C, S>(
    capture_source: &C,
    capture_id: &str,
    storage_service: &ArtifactStorageService<S>,
) -> Result<StoredArtifact>
where
    C: crate::CaptureSource,
    S: Storage,
{
    let snapshot_data = capture_source.snapshot().await?;
    storage_service
        .store_snapshot(capture_id, snapshot_data)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use gl_storage::{StorageConfig, StorageManager};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_artifact_storage_service() {
        let temp_dir = TempDir::new().unwrap();

        let storage_config = StorageConfig {
            base_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };

        let storage = StorageManager::new(storage_config).unwrap();

        let artifact_config = ArtifactStorageConfig {
            base_uri: format!("file://{}/artifacts", temp_dir.path().display()),
            snapshot_extension: "jpg".to_string(),
            include_timestamp: false,
        };

        let service = ArtifactStorageService::new(storage, artifact_config);

        let capture_id = "test_capture_123";
        let test_data = Bytes::from(b"fake JPEG data".to_vec());

        // Test storing snapshot
        let stored = service
            .store_snapshot(capture_id, test_data.clone())
            .await
            .unwrap();
        assert_eq!(stored.size, test_data.len());
        assert_eq!(stored.content_type, "image/jpeg");

        // Test retrieving snapshot
        let retrieved = service.get_artifact(&stored.uri).await.unwrap();
        assert_eq!(retrieved, test_data);

        // Test existence check
        assert!(service.artifact_exists(&stored.uri).await.unwrap());

        // Test deletion
        service.delete_artifact(&stored.uri).await.unwrap();
        assert!(!service.artifact_exists(&stored.uri).await.unwrap());
    }

    #[tokio::test]
    async fn test_filename_generation() {
        let storage_config = StorageConfig::default();
        let storage = StorageManager::new(storage_config).unwrap();

        let artifact_config = ArtifactStorageConfig {
            include_timestamp: false,
            ..Default::default()
        };

        let service = ArtifactStorageService::new(storage, artifact_config);

        let filename = service.generate_snapshot_filename("test123");
        assert_eq!(filename, "snapshot_test123.jpg");

        let recording_filename = service.generate_recording_filename("test456", "mp4");
        assert_eq!(recording_filename, "recording_test456.mp4");
    }

    #[tokio::test]
    async fn test_uri_building() {
        let storage_config = StorageConfig::default();
        let storage = StorageManager::new(storage_config).unwrap();

        let artifact_config = ArtifactStorageConfig {
            base_uri: "s3://mybucket/artifacts".to_string(),
            ..Default::default()
        };

        let service = ArtifactStorageService::new(storage, artifact_config);

        let uri = service.build_artifact_uri("test.jpg").unwrap();
        assert_eq!(uri.to_string(), "s3://mybucket/artifacts/test.jpg");
    }
}
