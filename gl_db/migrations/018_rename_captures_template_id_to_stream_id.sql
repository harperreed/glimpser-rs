-- Rename template_id to stream_id in captures table
-- This aligns with the elimination of the templates concept

-- Step 1: Drop the old index
DROP INDEX IF EXISTS idx_captures_template_id;

-- Step 2: Create new table with correct schema (SQLite doesn't support ALTER COLUMN)
CREATE TABLE captures_new (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    stream_id TEXT,
    name TEXT NOT NULL,
    description TEXT,
    source_url TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    config TEXT NOT NULL,
    metadata TEXT,
    file_path TEXT,
    file_size INTEGER,
    duration_seconds INTEGER,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (stream_id) REFERENCES streams(id) ON DELETE SET NULL
);

-- Step 3: Copy data from old table to new table
INSERT INTO captures_new (
    id, user_id, stream_id, name, description, source_url,
    status, config, metadata, file_path, file_size, duration_seconds,
    started_at, completed_at, created_at, updated_at
)
SELECT
    id, user_id, template_id as stream_id, name, description, source_url,
    status, config, metadata, file_path, file_size, duration_seconds,
    started_at, completed_at, created_at, updated_at
FROM captures;

-- Step 4: Drop old table
DROP TABLE captures;

-- Step 5: Rename new table to original name
ALTER TABLE captures_new RENAME TO captures;

-- Step 6: Recreate indexes with new names
CREATE INDEX idx_captures_user_id ON captures(user_id);
CREATE INDEX idx_captures_status ON captures(status);
CREATE INDEX idx_captures_created_at ON captures(created_at);
CREATE INDEX idx_captures_stream_id ON captures(stream_id);
