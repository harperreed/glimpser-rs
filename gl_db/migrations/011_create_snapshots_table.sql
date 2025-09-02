-- Create snapshots table for storing capture image data
CREATE TABLE snapshots (
    id TEXT PRIMARY KEY NOT NULL,
    template_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    image_data BLOB NOT NULL, -- JPEG image binary data
    content_type TEXT NOT NULL DEFAULT 'image/jpeg',
    width INTEGER,
    height INTEGER,
    file_size INTEGER NOT NULL,
    checksum TEXT, -- MD5 or SHA256 checksum
    captured_at TEXT NOT NULL, -- ISO8601 timestamp
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (template_id) REFERENCES templates(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Indexes for snapshot queries
CREATE INDEX idx_snapshots_template_id ON snapshots(template_id);
CREATE INDEX idx_snapshots_user_id ON snapshots(user_id);
CREATE INDEX idx_snapshots_captured_at ON snapshots(captured_at);
CREATE INDEX idx_snapshots_created_at ON snapshots(created_at);
