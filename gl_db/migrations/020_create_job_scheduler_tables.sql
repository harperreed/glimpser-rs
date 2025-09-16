-- Create scheduled_jobs table for the job scheduling system
CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    job_type TEXT NOT NULL,
    schedule TEXT NOT NULL,
    parameters TEXT NOT NULL, -- JSON
    enabled INTEGER NOT NULL DEFAULT 1,
    max_retries INTEGER NOT NULL DEFAULT 3,
    timeout_seconds INTEGER,
    priority INTEGER NOT NULL DEFAULT 0,
    tags TEXT, -- JSON array
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    metadata TEXT -- JSON
);

-- Create job_executions table for tracking job execution history
CREATE TABLE IF NOT EXISTS job_executions (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    duration_ms INTEGER,
    result TEXT, -- JSON
    error TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    executed_on TEXT,
    metadata TEXT, -- JSON
    FOREIGN KEY (job_id) REFERENCES scheduled_jobs(id) ON DELETE CASCADE
);

-- Indexes for scheduled jobs
CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_job_type ON scheduled_jobs(job_type);
CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_enabled ON scheduled_jobs(enabled);
CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_priority ON scheduled_jobs(priority DESC);
CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_created_by ON scheduled_jobs(created_by);
CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_created_at ON scheduled_jobs(created_at);

-- Indexes for job executions
CREATE INDEX IF NOT EXISTS idx_job_executions_job_id ON job_executions(job_id);
CREATE INDEX IF NOT EXISTS idx_job_executions_status ON job_executions(status);
CREATE INDEX IF NOT EXISTS idx_job_executions_started_at ON job_executions(started_at);
CREATE INDEX IF NOT EXISTS idx_job_executions_completed_at ON job_executions(completed_at);
