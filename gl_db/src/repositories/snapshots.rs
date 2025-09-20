//! ABOUTME: Snapshot repository for storing and retrieving capture image data
//! ABOUTME: Provides compile-time checked queries for snapshot CRUD operations with BLOB storage

use gl_core::{time::now_iso8601, Error, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::{debug, instrument};

/// Snapshot entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Snapshot {
    pub id: String,
    pub stream_id: String,
    pub user_id: String,
    pub file_path: String,
    pub storage_uri: String,
    pub content_type: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_size: i64,
    pub checksum: Option<String>,
    pub etag: Option<String>,
    pub captured_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub perceptual_hash: Option<String>,
}

/// Snapshot metadata (for listings)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SnapshotMetadata {
    pub id: String,
    pub stream_id: String,
    pub user_id: String,
    pub file_path: String,
    pub storage_uri: String,
    pub content_type: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_size: i64,
    pub checksum: Option<String>,
    pub etag: Option<String>,
    pub captured_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub perceptual_hash: Option<String>,
}

/// Request to create a new snapshot
#[derive(Debug, Clone)]
pub struct CreateSnapshotRequest {
    pub stream_id: String,
    pub user_id: String,
    pub file_path: String,
    pub storage_uri: String,
    pub content_type: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_size: i64,
    pub checksum: Option<String>,
    pub etag: Option<String>,
    pub captured_at: String,
    pub perceptual_hash: Option<String>,
}

/// Snapshot repository
pub struct SnapshotRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SnapshotRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new snapshot
    #[instrument(skip(self, request))]
    pub async fn create(&self, request: CreateSnapshotRequest) -> Result<Snapshot> {
        let id = Id::new().to_string();
        let now = now_iso8601();

        debug!(
            id = %id,
            stream_id = %request.stream_id,
            file_size = request.file_size,
            file_path = %request.file_path,
            "Creating snapshot"
        );

        sqlx::query!(
            r#"
            INSERT INTO snapshots (
                id, stream_id, user_id, file_path, storage_uri, content_type,
                width, height, file_size, checksum, etag, captured_at,
                created_at, updated_at, perceptual_hash
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            id,
            request.stream_id,
            request.user_id,
            request.file_path,
            request.storage_uri,
            request.content_type,
            request.width,
            request.height,
            request.file_size,
            request.checksum,
            request.etag,
            request.captured_at,
            now,
            now,
            request.perceptual_hash
        )
        .execute(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create snapshot: {}", e)))?;

        Ok(Snapshot {
            id,
            stream_id: request.stream_id,
            user_id: request.user_id,
            file_path: request.file_path,
            storage_uri: request.storage_uri,
            content_type: request.content_type,
            width: request.width,
            height: request.height,
            file_size: request.file_size,
            checksum: request.checksum,
            etag: request.etag,
            captured_at: request.captured_at,
            created_at: now.clone(),
            updated_at: now,
            perceptual_hash: request.perceptual_hash,
        })
    }

    /// Find snapshot by ID
    #[instrument(skip(self))]
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Snapshot>> {
        debug!(id = %id, "Finding snapshot by ID");

        let record = sqlx::query!(
            r#"
            SELECT id, stream_id, user_id, file_path, storage_uri, content_type,
                   width, height, file_size, checksum, etag, captured_at,
                   created_at, updated_at, perceptual_hash
            FROM snapshots
            WHERE id = ?
            "#,
            id
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find snapshot: {}", e)))?;

        Ok(record.map(|r| Snapshot {
            id: r.id,
            stream_id: r.stream_id,
            user_id: r.user_id,
            file_path: r.file_path,
            storage_uri: r.storage_uri,
            content_type: r.content_type,
            width: r.width,
            height: r.height,
            file_size: r.file_size,
            checksum: r.checksum,
            etag: r.etag,
            captured_at: r.captured_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
            perceptual_hash: r.perceptual_hash,
        }))
    }

    /// List snapshots for a template
    #[instrument(skip(self))]
    pub async fn list_by_template(
        &self,
        stream_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SnapshotMetadata>> {
        debug!(
            stream_id = %stream_id,
            limit = limit,
            offset = offset,
            "Listing snapshots for template"
        );

        let records = sqlx::query_as!(
            SnapshotMetadata,
            r#"
            SELECT id, stream_id, user_id, file_path, storage_uri, content_type,
                   width, height, file_size, checksum, etag, captured_at,
                   created_at, updated_at, perceptual_hash
            FROM snapshots
            WHERE stream_id = ?
            ORDER BY captured_at DESC
            LIMIT ? OFFSET ?
            "#,
            stream_id,
            limit,
            offset
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to list snapshots: {}", e)))?;

        Ok(records)
    }

    /// Get the latest snapshot for a template
    #[instrument(skip(self))]
    pub async fn get_latest_by_template(
        &self,
        stream_id: &str,
    ) -> Result<Option<SnapshotMetadata>> {
        debug!(stream_id = %stream_id, "Getting latest snapshot for template");

        let record = sqlx::query_as!(
            SnapshotMetadata,
            r#"
            SELECT id, stream_id, user_id, file_path, storage_uri, content_type,
                   width, height, file_size, checksum, etag, captured_at,
                   created_at, updated_at, perceptual_hash
            FROM snapshots
            WHERE stream_id = ?
            ORDER BY captured_at DESC
            LIMIT 1
            "#,
            stream_id
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get latest snapshot: {}", e)))?;

        Ok(record)
    }

    /// Delete old snapshots for a template (keep only the latest N)
    #[instrument(skip(self))]
    pub async fn cleanup_old_snapshots(&self, stream_id: &str, keep_count: i64) -> Result<i64> {
        debug!(
            stream_id = %stream_id,
            keep_count = keep_count,
            "Cleaning up old snapshots"
        );

        let result = sqlx::query!(
            r#"
            DELETE FROM snapshots
            WHERE stream_id = ?
            AND id NOT IN (
                SELECT id FROM snapshots
                WHERE stream_id = ?
                ORDER BY captured_at DESC
                LIMIT ?
            )
            "#,
            stream_id,
            stream_id,
            keep_count
        )
        .execute(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to cleanup old snapshots: {}", e)))?;

        Ok(result.rows_affected() as i64)
    }

    /// Count snapshots for a template
    #[instrument(skip(self))]
    pub async fn count_by_template(&self, stream_id: &str) -> Result<i64> {
        debug!(stream_id = %stream_id, "Counting snapshots for template");

        let record = sqlx::query!(
            "SELECT COUNT(*) as count FROM snapshots WHERE stream_id = ?",
            stream_id
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to count snapshots: {}", e)))?;

        Ok(record.count)
    }
}
