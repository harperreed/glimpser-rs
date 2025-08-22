-- Create jobs table for background processing and scheduling
CREATE TABLE jobs (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    capture_id TEXT,
    job_type TEXT NOT NULL, -- capture, process, analyze, cleanup, etc.
    priority INTEGER NOT NULL DEFAULT 0, -- Higher number = higher priority
    status TEXT NOT NULL DEFAULT 'pending', -- pending, running, completed, failed, cancelled
    payload TEXT NOT NULL, -- JSON job payload
    result TEXT, -- JSON job result
    error_message TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 3,
    scheduled_at TEXT NOT NULL, -- When to run the job
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE
);

-- Indexes for job processing
CREATE INDEX idx_jobs_status ON jobs(status);
CREATE INDEX idx_jobs_scheduled_at ON jobs(scheduled_at);
CREATE INDEX idx_jobs_priority ON jobs(priority DESC);
CREATE INDEX idx_jobs_user_id ON jobs(user_id);
CREATE INDEX idx_jobs_capture_id ON jobs(capture_id);
CREATE INDEX idx_jobs_type ON jobs(job_type);
