//! ABOUTME: Settings repository for storing and retrieving system configuration
//! ABOUTME: Provides type-safe access to configurable parameters like phash thresholds

use gl_core::{time::now_iso8601, Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::{debug, instrument};

/// Setting entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Setting {
    pub id: String,
    pub key: String,
    pub value: String,
    pub category: String,
    pub description: Option<String>,
    pub data_type: String,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub default_value: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to update a setting
#[derive(Debug, Clone)]
pub struct UpdateSettingRequest {
    pub key: String,
    pub value: String,
}

/// Settings repository
pub struct SettingsRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SettingsRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Get a setting value by key
    #[instrument(skip(self))]
    pub async fn get_value(&self, key: &str) -> Result<Option<String>> {
        debug!(key = %key, "Getting setting value");

        let record = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to get setting {}: {}", key, e)))?;

        Ok(record)
    }

    /// Get a setting value as float
    #[instrument(skip(self))]
    pub async fn get_float(&self, key: &str) -> Result<Option<f64>> {
        if let Some(value) = self.get_value(key).await? {
            value.parse::<f64>().map(Some).map_err(|e| {
                Error::Validation(format!("Setting {} is not a valid float: {}", key, e))
            })
        } else {
            Ok(None)
        }
    }

    /// Get a setting value as integer
    #[instrument(skip(self))]
    pub async fn get_int(&self, key: &str) -> Result<Option<i64>> {
        if let Some(value) = self.get_value(key).await? {
            value.parse::<i64>().map(Some).map_err(|e| {
                Error::Validation(format!("Setting {} is not a valid integer: {}", key, e))
            })
        } else {
            Ok(None)
        }
    }

    /// Get a setting value as boolean
    #[instrument(skip(self))]
    pub async fn get_bool(&self, key: &str) -> Result<Option<bool>> {
        if let Some(value) = self.get_value(key).await? {
            match value.to_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Ok(Some(true)),
                "false" | "0" | "no" | "off" => Ok(Some(false)),
                _ => Err(Error::Validation(format!(
                    "Setting {} is not a valid boolean: {}",
                    key, value
                ))),
            }
        } else {
            Ok(None)
        }
    }

    /// Get all settings in a category
    #[instrument(skip(self))]
    pub async fn get_by_category(&self, category: &str) -> Result<Vec<Setting>> {
        debug!(category = %category, "Getting settings by category");

        let records = sqlx::query_as!(
            Setting,
            r#"
            SELECT id as "id!", key as "key!", value as "value!", category as "category!",
                   description, data_type as "data_type!",
                   min_value, max_value, default_value,
                   created_at as "created_at!", updated_at as "updated_at!"
            FROM settings
            WHERE category = ?
            ORDER BY key
            "#,
            category
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to get settings for category {}: {}",
                category, e
            ))
        })?;

        Ok(records)
    }

    /// Get all settings
    #[instrument(skip(self))]
    pub async fn get_all(&self) -> Result<Vec<Setting>> {
        debug!("Getting all settings");

        let records = sqlx::query_as!(
            Setting,
            r#"
            SELECT id as "id!", key as "key!", value as "value!", category as "category!",
                   description, data_type as "data_type!",
                   min_value, max_value, default_value,
                   created_at as "created_at!", updated_at as "updated_at!"
            FROM settings
            ORDER BY category, key
            "#
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get all settings: {}", e)))?;

        Ok(records)
    }

    /// Update a setting value
    #[instrument(skip(self, request))]
    pub async fn update(&self, request: UpdateSettingRequest) -> Result<()> {
        debug!(key = %request.key, "Updating setting");

        let now = now_iso8601();

        let result = sqlx::query!(
            "UPDATE settings SET value = ?, updated_at = ? WHERE key = ?",
            request.value,
            now,
            request.key
        )
        .execute(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update setting {}: {}", request.key, e)))?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound(format!(
                "Setting {} not found",
                request.key
            )));
        }

        Ok(())
    }

    /// Get the perceptual hash similarity threshold (convenience method)
    #[instrument(skip(self))]
    pub async fn get_phash_threshold(&self) -> Result<f64> {
        self.get_float("phash_similarity_threshold")
            .await?
            .ok_or_else(|| Error::Config("phash_similarity_threshold not configured".to_string()))
    }

    /// Get snapshot retention count (convenience method)
    #[instrument(skip(self))]
    pub async fn get_snapshot_retention(&self) -> Result<i64> {
        self.get_int("snapshot_retention_count")
            .await?
            .ok_or_else(|| Error::Config("snapshot_retention_count not configured".to_string()))
    }
}
