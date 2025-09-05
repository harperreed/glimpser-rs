-- Rename template_id to stream_id in snapshots table
-- This aligns with the elimination of the templates concept

-- Step 1: Drop the foreign key constraint and old indexes
DROP INDEX IF EXISTS idx_snapshots_template_id;

-- Step 2: Rename the column (SQLite doesn't support ALTER COLUMN, so we need to recreate)
-- Create new table with correct schema
CREATE TABLE snapshots_new (
    id TEXT PRIMARY KEY NOT NULL,
    stream_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    storage_uri TEXT NOT NULL,
    content_type TEXT NOT NULL DEFAULT 'image/jpeg',
    width INTEGER,
    height INTEGER,
    file_size INTEGER NOT NULL,
    checksum TEXT,
    etag TEXT,
    captured_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (stream_id) REFERENCES streams(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Step 3: Copy data from old table to new table
INSERT INTO snapshots_new (
    id, stream_id, user_id, file_path, storage_uri,
    content_type, width, height, file_size, checksum, etag,
    captured_at, created_at, updated_at
)
SELECT
    id, template_id as stream_id, user_id, file_path, storage_uri,
    content_type, width, height, file_size, checksum, etag,
    captured_at, created_at, updated_at
FROM snapshots;

-- Step 4: Drop old table
DROP TABLE snapshots;

-- Step 5: Rename new table to original name
ALTER TABLE snapshots_new RENAME TO snapshots;

-- Step 6: Recreate indexes with new names
CREATE INDEX idx_snapshots_stream_id ON snapshots(stream_id);
CREATE INDEX idx_snapshots_user_id ON snapshots(user_id);
CREATE INDEX idx_snapshots_captured_at ON snapshots(captured_at);
CREATE INDEX idx_snapshots_created_at ON snapshots(created_at);
