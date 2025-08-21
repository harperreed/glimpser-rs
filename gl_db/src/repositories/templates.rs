//! ABOUTME: Template repository for managing capture configuration templates
//! ABOUTME: Provides compile-time checked queries for template CRUD operations

use gl_core::{Result, Error, time::now_iso8601, Id};
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
        let template = sqlx::query_as!(
            Template,
            "SELECT * FROM templates WHERE id = ?1",
            id
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find template: {}", e)))?;
        
        Ok(template)
    }
}