//! ABOUTME: Cache-aware user repository wrapper for improved performance
//! ABOUTME: Provides caching layer over UserRepository for frequently accessed user data

use crate::{cache::DatabaseCache, repositories::users::*};
use gl_core::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::debug;

/// Cache-aware user repository that wraps UserRepository with caching logic
pub struct CachedUserRepository<'a> {
    repo: UserRepository<'a>,
    cache: Arc<DatabaseCache>,
}

impl<'a> CachedUserRepository<'a> {
    pub fn new(pool: &'a SqlitePool, cache: Arc<DatabaseCache>) -> Self {
        Self {
            repo: UserRepository::new(pool),
            cache,
        }
    }

    /// Create a new user and cache it
    pub async fn create(&self, request: CreateUserRequest) -> Result<User> {
        debug!("Creating user with caching: {}", request.username);

        let user = self.repo.create(request).await?;

        // Cache the new user
        self.cache.cache_user(user.clone());

        debug!("Cached new user: {}", user.id);
        Ok(user)
    }

    /// Find user by ID with cache lookup first
    pub async fn find_by_id(&self, id: &str) -> Result<Option<User>> {
        debug!("Finding user by id with caching: {}", id);

        // Check cache first
        if let Some(cached_user) = self.cache.get_user(id) {
            debug!("User cache hit for id: {}", id);
            return Ok(Some(cached_user));
        }

        // Cache miss - fetch from database
        debug!("User cache miss for id: {}", id);
        let user = self.repo.find_by_id(id).await?;

        // Cache the result if found
        if let Some(ref user) = user {
            self.cache.cache_user(user.clone());
            debug!("Cached user from database: {}", user.id);
        }

        Ok(user)
    }

    /// Find user by username - delegates to database (less frequently used)
    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        debug!("Finding user by username: {}", username);

        let user = self.repo.find_by_username(username).await?;

        // Cache the result if found
        if let Some(ref user) = user {
            self.cache.cache_user(user.clone());
            debug!("Cached user from username lookup: {}", user.id);
        }

        Ok(user)
    }

    /// Find user by email with cache lookup first (login optimization)
    pub async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
        debug!("Finding user by email with caching: {}", email);

        // Check cache first
        if let Some(cached_user) = self.cache.get_user_by_email(email) {
            debug!("User email cache hit for: {}", email);
            return Ok(Some(cached_user));
        }

        // Cache miss - fetch from database
        debug!("User email cache miss for: {}", email);
        let user = self.repo.find_by_email(email).await?;

        // Cache the result if found
        if let Some(ref user) = user {
            self.cache.cache_user(user.clone());
            debug!("Cached user from email lookup: {}", user.id);
        }

        Ok(user)
    }

    /// List active users - delegates to database (not cached due to list nature)
    pub async fn list_active(&self) -> Result<Vec<User>> {
        self.repo.list_active().await
    }

    /// Update user and invalidate cache
    pub async fn update(&self, id: &str, request: UpdateUserRequest) -> Result<User> {
        debug!("Updating user with cache invalidation: {}", id);

        // Get current user for email if needed
        let current_email = if let Ok(Some(current_user)) = self.repo.find_by_id(id).await {
            Some(current_user.email)
        } else {
            None
        };

        // Update in database
        let updated_user = self.repo.update(id, request).await?;

        // Invalidate cache entries
        self.cache.invalidate_user(id, current_email.as_deref());

        // Cache the updated user
        self.cache.cache_user(updated_user.clone());

        debug!("Updated and re-cached user: {}", updated_user.id);
        Ok(updated_user)
    }

    /// Delete user and invalidate cache
    pub async fn delete(&self, id: &str) -> Result<()> {
        debug!("Deleting user with cache invalidation: {}", id);

        // Get current user for email before deletion
        let current_email = if let Ok(Some(current_user)) = self.repo.find_by_id(id).await {
            Some(current_user.email)
        } else {
            None
        };

        // Delete from database
        self.repo.delete(id).await?;

        // Invalidate cache entries
        self.cache.invalidate_user(id, current_email.as_deref());

        debug!("Deleted and invalidated user cache: {}", id);
        Ok(())
    }
}
