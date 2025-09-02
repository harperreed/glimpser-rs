-- Convert snapshots table from BLOB storage to file path storage
-- Drop the existing table and recreate with file_path instead of image_data

DROP TABLE snapshots;

-- Recreate snapshots table with file path storage
CREATE TABLE snapshots (
    id TEXT PRIMARY KEY NOT NULL,
    template_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    file_path TEXT NOT NULL, -- Path to stored image file
    storage_uri TEXT NOT NULL, -- Full storage URI (e.g., file:///data/artifacts/snapshot_123.jpg)
    content_type TEXT NOT NULL DEFAULT 'image/jpeg',
    width INTEGER,
    height INTEGER,
    file_size INTEGER NOT NULL,
    checksum TEXT, -- MD5 or SHA256 checksum
    etag TEXT, -- Storage ETag if available
    captured_at TEXT NOT NULL, -- ISO8601 timestamp
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (template_id) REFERENCES templates(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Recreate indexes for snapshot queries
CREATE INDEX idx_snapshots_template_id ON snapshots(template_id);
CREATE INDEX idx_snapshots_user_id ON snapshots(user_id);
CREATE INDEX idx_snapshots_captured_at ON snapshots(captured_at);
CREATE INDEX idx_snapshots_created_at ON snapshots(created_at);
CREATE INDEX idx_snapshots_file_path ON snapshots(file_path);
