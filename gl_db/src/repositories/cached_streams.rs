//! ABOUTME: Cache-aware stream repository wrapper for improved performance
//! ABOUTME: Provides caching layer over StreamRepository for frequently accessed stream data

use crate::{cache::DatabaseCache, repositories::streams::*};
use gl_core::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::debug;

/// Cache-aware stream repository that wraps StreamRepository with caching logic
pub struct CachedStreamRepository<'a> {
    repo: StreamRepository<'a>,
    cache: Arc<DatabaseCache>,
}

impl<'a> CachedStreamRepository<'a> {
    pub fn new(pool: &'a SqlitePool, cache: Arc<DatabaseCache>) -> Self {
        Self {
            repo: StreamRepository::new(pool),
            cache,
        }
    }

    /// Create a new stream and cache it
    pub async fn create(&self, request: CreateStreamRequest) -> Result<Stream> {
        debug!("Creating stream with caching: {}", request.name);

        let stream = self.repo.create(request).await?;

        // Cache the new stream
        self.cache.cache_stream(stream.clone());

        debug!("Cached new stream: {}", stream.id);
        Ok(stream)
    }

    /// Find stream by ID with cache lookup first
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Stream>> {
        debug!("Finding stream by id with caching: {}", id);

        // Check cache first
        if let Some(cached_stream) = self.cache.get_stream(id) {
            debug!("Stream cache hit for id: {}", id);
            return Ok(Some(cached_stream));
        }

        // Cache miss - fetch from database
        debug!("Stream cache miss for id: {}", id);
        let stream = self.repo.find_by_id(id).await?;

        // Cache the result if found
        if let Some(ref stream) = stream {
            self.cache.cache_stream(stream.clone());
            debug!("Cached stream from database: {}", stream.id);
        }

        Ok(stream)
    }

    /// Update stream and invalidate cache
    pub async fn update(&self, id: &str, request: UpdateStreamRequest) -> Result<Option<Stream>> {
        debug!("Updating stream with cache invalidation: {}", id);

        // Update in database
        let updated_stream = self.repo.update(id, request).await?;

        // Invalidate cache entry
        self.cache.invalidate_stream(id);

        // Cache the updated stream if it exists
        if let Some(ref stream) = updated_stream {
            self.cache.cache_stream(stream.clone());
            debug!("Updated and re-cached stream: {}", stream.id);
        }

        Ok(updated_stream)
    }

    /// Delete stream and invalidate cache
    pub async fn delete(&self, id: &str) -> Result<bool> {
        debug!("Deleting stream with cache invalidation: {}", id);

        // Delete from database
        let deleted = self.repo.delete(id).await?;

        // Invalidate cache entry if deletion was successful
        if deleted {
            self.cache.invalidate_stream(id);
            debug!("Deleted and invalidated stream cache: {}", id);
        }

        Ok(deleted)
    }

    /// List streams - delegates to database (not cached due to list nature)
    pub async fn list(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Stream>> {
        self.repo.list(user_id, offset, limit).await
    }

    /// Update execution status and invalidate cache
    pub async fn update_execution_status(
        &self,
        id: &str,
        status: &str,
        executed_at: Option<&str>,
    ) -> Result<bool> {
        debug!(
            "Updating stream execution status with cache invalidation: {}",
            id
        );

        let updated = self
            .repo
            .update_execution_status(id, status, executed_at)
            .await?;

        // Invalidate cache since execution status changed
        if updated {
            self.cache.invalidate_stream(id);
            debug!(
                "Updated execution status and invalidated cache for stream: {}",
                id
            );
        }

        Ok(updated)
    }

    /// Find by name and user - delegates to database (less frequently used)
    pub async fn find_by_name_and_user(&self, name: &str, user_id: &str) -> Result<Option<Stream>> {
        debug!(
            "Finding stream by name and user: {} (user: {})",
            name, user_id
        );

        let stream = self.repo.find_by_name_and_user(name, user_id).await?;

        // Cache the result if found
        if let Some(ref stream) = stream {
            self.cache.cache_stream(stream.clone());
            debug!("Cached stream from name/user lookup: {}", stream.id);
        }

        Ok(stream)
    }

    /// List streams with total count - optimized compound query (not cached due to pagination)
    pub async fn list_with_total(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<Stream>, i64)> {
        debug!(
            "Listing streams with total count: user_id={:?}, offset={}, limit={}",
            user_id, offset, limit
        );

        // Use the optimized compound query to eliminate N+1 pattern
        let (streams, total) = self.repo.list_with_total(user_id, offset, limit).await?;

        // Cache individual streams for future lookups, but limit to avoid cache pollution
        // Only cache if result set is reasonably small to prevent memory issues
        if streams.len() <= 50 {
            for stream in &streams {
                self.cache.cache_stream(stream.clone());
            }
            debug!("Listed and cached {} streams from database", streams.len());
        } else {
            debug!(
                "Listed {} streams (skipped caching due to large result set)",
                streams.len()
            );
        }
        Ok((streams, total))
    }

    /// Search streams with total count - optimized compound query (not cached due to search nature)
    pub async fn search_with_total(
        &self,
        user_id: Option<&str>,
        name_pattern: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<Stream>, i64)> {
        debug!(
            "Searching streams with total count: user_id={:?}, pattern={}, offset={}, limit={}",
            user_id, name_pattern, offset, limit
        );

        // Use the optimized compound query to eliminate N+1 pattern
        let (streams, total) = self
            .repo
            .search_with_total(user_id, name_pattern, offset, limit)
            .await?;

        // Cache individual streams for future lookups, but limit to avoid cache pollution
        // Only cache if result set is reasonably small to prevent memory issues
        if streams.len() <= 50 {
            for stream in &streams {
                self.cache.cache_stream(stream.clone());
            }
            debug!(
                "Searched and cached {} streams from database",
                streams.len()
            );
        } else {
            debug!(
                "Searched {} streams (skipped caching due to large result set)",
                streams.len()
            );
        }
        Ok((streams, total))
    }

    /// Count streams - delegates to database (used less frequently)
    pub async fn count(&self, user_id: Option<&str>) -> Result<i64> {
        self.repo.count(user_id).await
    }

    /// Search by name pattern - delegates to database (used less frequently)
    pub async fn search_by_name(
        &self,
        name_pattern: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Stream>> {
        let streams = self
            .repo
            .search_by_name(name_pattern, offset, limit)
            .await?;

        // Cache individual streams for future lookups, but limit to avoid cache pollution
        // Only cache if result set is reasonably small to prevent memory issues
        if streams.len() <= 50 {
            for stream in &streams {
                self.cache.cache_stream(stream.clone());
            }
            debug!("Searched by name and cached {} streams", streams.len());
        } else {
            debug!(
                "Searched by name {} streams (skipped caching due to large result set)",
                streams.len()
            );
        }
        Ok(streams)
    }

    /// Update execution status with error and invalidate cache
    pub async fn update_execution_status_with_error(
        &self,
        id: &str,
        status: &str,
        error_message: &str,
    ) -> Result<bool> {
        debug!(
            "Updating stream execution status with error and cache invalidation: {}",
            id
        );

        let updated = self
            .repo
            .update_execution_status_with_error(id, status, error_message)
            .await?;

        // Invalidate cache since execution status changed
        if updated {
            self.cache.invalidate_stream(id);
            debug!(
                "Updated execution status with error and invalidated cache for stream: {}",
                id
            );
        }

        Ok(updated)
    }

    /// Reset stale active statuses - delegates to database (infrequent operation)
    pub async fn reset_stale_active_statuses(&self) -> Result<()> {
        debug!("Resetting stale active statuses with cache invalidation");

        self.repo.reset_stale_active_statuses().await?;

        // Clear all cached streams since we don't know which ones were affected
        // This is safe since it's called on startup and affects multiple streams
        debug!("Clearing all stream cache after stale status reset");
        self.cache.clear_streams();

        Ok(())
    }
}
