//! ABOUTME: Event repository for audit logging and system event tracking
//! ABOUTME: Provides compile-time checked queries for event logging operations

use gl_core::{Result, Error, time::now_iso8601, Id};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

/// Event entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Event {
    pub id: String,
    pub user_id: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub event_type: String,
    pub details: Option<String>, // JSON
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: String,
}

/// Request to create a new event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEventRequest {
    pub user_id: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub event_type: String,
    pub details: Option<String>, // JSON
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

/// Event repository
pub struct EventRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> EventRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new event
    pub async fn create(&self, request: CreateEventRequest) -> Result<Event> {
        let id = Id::new().to_string();
        let now = now_iso8601();
        
        let event = sqlx::query_as!(
            Event,
            r#"
            INSERT INTO events (id, user_id, entity_type, entity_id, event_type, details, ip_address, user_agent, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            RETURNING *
            "#,
            id,
            request.user_id,
            request.entity_type,
            request.entity_id,
            request.event_type,
            request.details,
            request.ip_address,
            request.user_agent,
            now
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create event: {}", e)))?;
        
        Ok(event)
    }

    /// Find event by ID
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Event>> {
        let event = sqlx::query_as!(
            Event,
            "SELECT * FROM events WHERE id = ?1",
            id
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find event: {}", e)))?;
        
        Ok(event)
    }
}