//! ABOUTME: User repository with authentication and user management operations
//! ABOUTME: Provides compile-time checked queries for user CRUD operations

use gl_core::{time::now_iso8601, Error, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::{debug, instrument};

/// User entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub is_active: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub password_hash: String,
}

/// Request to update a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub is_active: Option<bool>,
}

/// User repository
pub struct UserRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> UserRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new user
    #[instrument(skip(self, request))]
    pub async fn create(&self, request: CreateUserRequest) -> Result<User> {
        let id = Id::new().to_string();
        let now = now_iso8601();

        debug!("Creating user with id: {}", id);

        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (id, username, email, password_hash, is_active, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, true, ?5, ?6)
            RETURNING id, username, email, password_hash, is_active, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(request.username)
        .bind(request.email)
        .bind(request.password_hash)
        .bind(&now)
        .bind(&now)
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create user: {}", e)))?;

        debug!("Successfully created user: {}", user.id);
        Ok(user)
    }

    /// Find a user by ID
    #[instrument(skip(self))]
    pub async fn find_by_id(&self, id: &str) -> Result<Option<User>> {
        debug!("Finding user by id: {}", id);

        let user = sqlx::query_as!(User,
            "SELECT id, username, email, password_hash, is_active, created_at, updated_at FROM users WHERE id = ?1",
            id)
            .fetch_optional(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to find user by id: {}", e)))?;

        Ok(user)
    }

    /// Find a user by username
    #[instrument(skip(self))]
    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        debug!("Finding user by username: {}", username);

        let user = sqlx::query_as!(User,
            "SELECT id, username, email, password_hash, is_active, created_at, updated_at FROM users WHERE username = ?1",
            username)
            .fetch_optional(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to find user by username: {}", e)))?;

        Ok(user)
    }

    /// Find a user by email
    #[instrument(skip(self))]
    pub async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
        debug!("Finding user by email: {}", email);

        let user = sqlx::query_as!(User,
            "SELECT id, username, email, password_hash, is_active, created_at, updated_at FROM users WHERE email = ?1",
            email)
            .fetch_optional(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to find user by email: {}", e)))?;

        Ok(user)
    }

    /// List all active users
    #[instrument(skip(self))]
    pub async fn list_active(&self) -> Result<Vec<User>> {
        debug!("Listing active users");

        let users = sqlx::query_as!(
            User,
            "SELECT id, username, email, password_hash, is_active, created_at, updated_at FROM users WHERE is_active = true ORDER BY created_at DESC"
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to list active users: {}", e)))?;

        debug!("Found {} active users", users.len());
        Ok(users)
    }

    /// Update a user
    #[instrument(skip(self, request))]
    pub async fn update(&self, id: &str, request: UpdateUserRequest) -> Result<User> {
        debug!("Updating user: {}", id);

        // Validate that at least one field is provided
        if request.username.is_none()
            && request.email.is_none()
            && request.password_hash.is_none()
            && request.is_active.is_none()
        {
            return Err(Error::Validation("No fields to update".to_string()));
        }

        let now = now_iso8601();

        // Use transaction to ensure atomicity of read-then-write operation
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(format!("Failed to begin transaction: {}", e)))?;

        // Get current user to preserve unchanged fields within transaction
        let current_user = sqlx::query_as!(
            User,
            "SELECT id, username, email, password_hash, is_active, created_at, updated_at FROM users WHERE id = ?1",
            id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to find user by id: {}", e)))?
        .ok_or_else(|| Error::NotFound("User not found".to_string()))?;

        // Use provided values or fall back to current values
        let username = request.username.unwrap_or(current_user.username);
        let email = request.email.unwrap_or(current_user.email);
        let password_hash = request.password_hash.unwrap_or(current_user.password_hash);
        let is_active = request
            .is_active
            .unwrap_or(current_user.is_active.unwrap_or(true));

        // Single update query with all fields within transaction
        let user = sqlx::query_as!(
            User,
            r#"
            UPDATE users
            SET username = ?1, email = ?2, password_hash = ?3, is_active = ?4, updated_at = ?5
            WHERE id = ?6
            RETURNING id, username, email, password_hash, is_active, created_at, updated_at
            "#,
            username,
            email,
            password_hash,
            is_active,
            now,
            id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to update user: {}", e)))?;

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| Error::Database(format!("Failed to commit transaction: {}", e)))?;

        debug!("Successfully updated user: {}", user.id);
        Ok(user)
    }

    /// Delete a user (soft delete - mark as inactive)
    #[instrument(skip(self))]
    pub async fn delete(&self, id: &str) -> Result<()> {
        debug!("Soft deleting user: {}", id);

        let now = now_iso8601();

        let result = sqlx::query!(
            "UPDATE users SET is_active = false, updated_at = ?1 WHERE id = ?2",
            now,
            id
        )
        .execute(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete user: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound("User not found".to_string()));
        }

        debug!("Successfully deleted user: {}", id);
        Ok(())
    }

    /// Check if any active users exist in the database
    #[instrument(skip(self))]
    pub async fn has_any_users(&self) -> Result<bool> {
        debug!("Checking if any users exist");

        let count_result =
            sqlx::query!("SELECT COUNT(*) as count FROM users WHERE is_active = true")
                .fetch_one(self.pool)
                .await
                .map_err(|e| Error::Database(format!("Failed to count users: {}", e)))?;

        let has_users = count_result.count > 0;
        debug!("User count check: {}", has_users);
        Ok(has_users)
    }
}
