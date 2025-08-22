//! ABOUTME: Template repository for managing capture configuration templates
//! ABOUTME: Provides compile-time checked queries for template CRUD operations

use gl_core::{time::now_iso8601, Error, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

/// Template entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Template {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub config: String, // JSON
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTemplateRequest {
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub config: String, // JSON
    pub is_default: bool,
}

/// Request to update a template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTemplateRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub config: Option<String>,
    pub is_default: Option<bool>,
}

/// Template repository
pub struct TemplateRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> TemplateRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new template
    pub async fn create(&self, request: CreateTemplateRequest) -> Result<Template> {
        let id = Id::new().to_string();
        let now = now_iso8601();

        let template = sqlx::query_as!(
            Template,
            r#"
            INSERT INTO templates (id, user_id, name, description, config, is_default, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            RETURNING *
            "#,
            id,
            request.user_id,
            request.name,
            request.description,
            request.config,
            request.is_default,
            now,
            now
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create template: {}", e)))?;

        Ok(template)
    }

    /// Find template by ID
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Template>> {
        let template = sqlx::query_as!(Template, "SELECT * FROM templates WHERE id = ?1", id)
            .fetch_optional(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to find template: {}", e)))?;

        Ok(template)
    }

    /// Update template
    pub async fn update(
        &self,
        id: &str,
        request: UpdateTemplateRequest,
    ) -> Result<Option<Template>> {
        let now = now_iso8601();

        // Build dynamic query based on provided fields
        let existing = match self.find_by_id(id).await? {
            Some(template) => template,
            None => return Ok(None),
        };

        let name = request.name.unwrap_or(existing.name);
        let description = request.description.or(existing.description);
        let config = request.config.unwrap_or(existing.config);
        let is_default = request.is_default.unwrap_or(existing.is_default);

        let template = sqlx::query_as!(
            Template,
            r#"
            UPDATE templates
            SET name = ?1, description = ?2, config = ?3, is_default = ?4, updated_at = ?5
            WHERE id = ?6
            RETURNING *
            "#,
            name,
            description,
            config,
            is_default,
            now,
            id
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update template: {}", e)))?;

        Ok(template)
    }

    /// Delete template
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query!("DELETE FROM templates WHERE id = ?1", id)
            .execute(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to delete template: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// List templates with pagination and filtering
    pub async fn list(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Template>> {
        let templates = if let Some(uid) = user_id {
            sqlx::query_as!(
                Template,
                "SELECT * FROM templates WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                uid,
                limit,
                offset
            )
            .fetch_all(self.pool)
            .await
        } else {
            sqlx::query_as!(
                Template,
                "SELECT * FROM templates ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                limit,
                offset
            )
            .fetch_all(self.pool)
            .await
        };

        templates.map_err(|e| Error::Database(format!("Failed to list templates: {}", e)))
    }

    /// Count templates for pagination
    pub async fn count(&self, user_id: Option<&str>) -> Result<i64> {
        let count = if let Some(uid) = user_id {
            sqlx::query_scalar!("SELECT COUNT(*) FROM templates WHERE user_id = ?1", uid)
                .fetch_one(self.pool)
                .await
        } else {
            sqlx::query_scalar!("SELECT COUNT(*) FROM templates")
                .fetch_one(self.pool)
                .await
        };

        count.map_err(|e| Error::Database(format!("Failed to count templates: {}", e)))
    }

    /// Find templates by name (search)
    pub async fn search_by_name(
        &self,
        name_pattern: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Template>> {
        let pattern = format!("%{}%", name_pattern);
        let templates = sqlx::query_as!(
            Template,
            "SELECT * FROM templates WHERE name LIKE ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            pattern,
            limit,
            offset
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to search templates: {}", e)))?;

        Ok(templates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::create_test_db;
    use crate::{CreateUserRequest, UserRepository};
    use gl_core::Id;

    #[tokio::test]
    async fn test_template_crud_operations() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");
        let repo = TemplateRepository::new(db.pool());

        // Create a test user first (templates have foreign key constraint)
        let user_repo = UserRepository::new(db.pool());
        let test_user = user_repo
            .create(CreateUserRequest {
                username: "testuser".to_string(),
                email: "test@example.com".to_string(),
                password_hash: "hashed_password".to_string(),
                role: "admin".to_string(),
            })
            .await
            .expect("Failed to create test user");

        // Create a test template
        let create_request = CreateTemplateRequest {
            user_id: test_user.id.clone(),
            name: "Test Template".to_string(),
            description: Some("A test template".to_string()),
            config: r#"{"kind": "ffmpeg", "source_url": "rtsp://test/stream"}"#.to_string(),
            is_default: false,
        };

        let template = repo.create(create_request).await.unwrap();
        assert_eq!(template.name, "Test Template");
        assert_eq!(template.description, Some("A test template".to_string()));
        assert!(!template.is_default);

        // Find by ID
        let found = repo.find_by_id(&template.id).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id, template.id);
        assert_eq!(found.name, "Test Template");

        // Update template
        let update_request = UpdateTemplateRequest {
            name: Some("Updated Template".to_string()),
            description: Some("Updated description".to_string()),
            config: Some(r#"{"kind": "file", "file_path": "/test/path"}"#.to_string()),
            is_default: Some(true),
        };

        let updated = repo.update(&template.id, update_request).await.unwrap();
        assert!(updated.is_some());
        let updated = updated.unwrap();
        assert_eq!(updated.name, "Updated Template");
        assert_eq!(updated.description, Some("Updated description".to_string()));
        assert!(updated.is_default);

        // List templates
        let templates = repo.list(None, 0, 10).await.unwrap();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].id, template.id);

        // Count templates
        let count = repo.count(None).await.unwrap();
        assert_eq!(count, 1);

        // Delete template
        let deleted = repo.delete(&template.id).await.unwrap();
        assert!(deleted);

        // Verify deletion
        let not_found = repo.find_by_id(&template.id).await.unwrap();
        assert!(not_found.is_none());

        let count_after_delete = repo.count(None).await.unwrap();
        assert_eq!(count_after_delete, 0);
    }
}
