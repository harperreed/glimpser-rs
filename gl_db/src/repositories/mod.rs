//! ABOUTME: Repository modules providing type-safe database operations
//! ABOUTME: Each repository handles CRUD operations for specific entity types

pub mod alerts;
pub mod analysis_events;
pub mod api_keys;
pub mod captures;
pub mod events;
pub mod jobs;
pub mod notification_deliveries;
pub mod settings;
pub mod snapshots;
pub mod streams;
pub mod users;

// Cache-aware repositories
pub mod cached_streams;
pub mod cached_users;
