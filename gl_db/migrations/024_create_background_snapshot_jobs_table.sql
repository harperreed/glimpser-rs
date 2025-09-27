-- Create background_snapshot_jobs table for tracking FFmpeg snapshot operations
CREATE TABLE IF NOT EXISTS background_snapshot_jobs (
    id TEXT PRIMARY KEY,
    input_path TEXT NOT NULL,
    stream_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending', -- pending, processing, completed, failed, cancelled
    config TEXT NOT NULL, -- JSON serialized SnapshotConfig
    result_size INTEGER,
    error_message TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    duration_ms INTEGER,
    created_by TEXT,
    metadata TEXT -- JSON for additional context
);

-- Indexes for background snapshot jobs
CREATE INDEX IF NOT EXISTS idx_background_snapshot_jobs_status ON background_snapshot_jobs(status);
CREATE INDEX IF NOT EXISTS idx_background_snapshot_jobs_stream_id ON background_snapshot_jobs(stream_id);
CREATE INDEX IF NOT EXISTS idx_background_snapshot_jobs_created_at ON background_snapshot_jobs(created_at);
CREATE INDEX IF NOT EXISTS idx_background_snapshot_jobs_completed_at ON background_snapshot_jobs(completed_at);
CREATE INDEX IF NOT EXISTS idx_background_snapshot_jobs_created_by ON background_snapshot_jobs(created_by);

-- Index for cleanup queries (finding old completed jobs)
CREATE INDEX IF NOT EXISTS idx_background_snapshot_jobs_cleanup ON background_snapshot_jobs(status, completed_at);
