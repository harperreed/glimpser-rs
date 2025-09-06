-- Add missing database indexes for performance optimization

-- Compound index for API key lookups with active status
-- This optimizes the common pattern: key_hash = ? AND is_active = true
CREATE INDEX IF NOT EXISTS idx_api_keys_hash_active
ON api_keys(key_hash, is_active);

-- Index for notification deliveries created_at for time-based queries
-- This optimizes: WHERE created_at >= datetime('now', '-' || ? || ' hours')
CREATE INDEX IF NOT EXISTS idx_notification_deliveries_created_at
ON notification_deliveries(created_at);

-- Compound index for notification deliveries status and retry logic
-- This optimizes retry queries with status and attempt count
CREATE INDEX IF NOT EXISTS idx_notification_deliveries_status_attempts
ON notification_deliveries(status, attempt_count, scheduled_at);

-- Note: analysis_events table still uses template_id (not yet migrated to stream_id)
-- The existing idx_analysis_events_template_id index should be sufficient
-- but we can add a compound index for better performance

-- Compound index for better analysis event filtering with time
-- This optimizes queries that filter by both template and time
CREATE INDEX IF NOT EXISTS idx_analysis_events_template_time
ON analysis_events(template_id, created_at);
