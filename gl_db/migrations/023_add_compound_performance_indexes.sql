-- Add compound database indexes for improved query performance
-- Based on backend-frontend performance optimization plan

-- Compound index for stream list queries: user_id + created_at (for pagination)
-- This optimizes the common pattern: WHERE user_id = ? ORDER BY created_at DESC
CREATE INDEX IF NOT EXISTS idx_streams_user_created
ON streams(user_id, created_at DESC);

-- Compound index for stream name search with user filtering
-- This optimizes: WHERE user_id = ? AND name LIKE ?
-- Equality check (user_id) comes first for optimal performance
CREATE INDEX IF NOT EXISTS idx_streams_name_user
ON streams(user_id, name) WHERE name IS NOT NULL;

-- Compound index for captures by stream and time (for snapshot queries)
-- This optimizes: WHERE stream_id = ? ORDER BY created_at DESC
CREATE INDEX IF NOT EXISTS idx_captures_stream_created
ON captures(stream_id, created_at DESC);

-- Compound index for snapshots by stream and time
-- This optimizes recent snapshot lookups by stream
CREATE INDEX IF NOT EXISTS idx_snapshots_stream_created
ON snapshots(stream_id, created_at DESC);

-- Note: These compound indexes will significantly improve performance for:
-- 1. Stream listing with user filtering and pagination
-- 2. Stream search within user context
-- 3. Recent snapshot/capture queries by stream
-- 4. Time-based queries that are common in surveillance systems
