-- Create captures table for capture metadata and results
CREATE TABLE captures (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    template_id TEXT,
    name TEXT NOT NULL,
    description TEXT,
    source_url TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending', -- pending, active, completed, failed, cancelled
    config TEXT NOT NULL, -- JSON capture configuration
    metadata TEXT, -- JSON metadata from capture
    file_path TEXT, -- Path to captured file/stream
    file_size INTEGER,
    duration_seconds INTEGER,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (template_id) REFERENCES templates(id) ON DELETE SET NULL
);

-- Indexes for capture queries
CREATE INDEX idx_captures_user_id ON captures(user_id);
CREATE INDEX idx_captures_status ON captures(status);
CREATE INDEX idx_captures_created_at ON captures(created_at);
CREATE INDEX idx_captures_template_id ON captures(template_id);