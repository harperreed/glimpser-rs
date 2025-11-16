-- Create job_locks table for distributed locking
-- Prevents duplicate job execution across multiple application instances
CREATE TABLE IF NOT EXISTS job_locks (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    execution_id TEXT NOT NULL,
    instance_id TEXT NOT NULL,
    locked_at TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'acquired', -- acquired, released, expired
    released_at TEXT,
    FOREIGN KEY (job_id) REFERENCES scheduled_jobs(id) ON DELETE CASCADE
);

-- CRITICAL: Unique constraint to prevent multiple active locks for the same job
-- This prevents race conditions where two instances try to acquire a lock simultaneously
CREATE UNIQUE INDEX IF NOT EXISTS idx_job_locks_unique_active
ON job_locks(job_id) WHERE status = 'acquired';

-- Index for quickly finding active locks for a job
CREATE INDEX IF NOT EXISTS idx_job_locks_job_id_status
ON job_locks(job_id, status);

-- Index for finding locks by instance
CREATE INDEX IF NOT EXISTS idx_job_locks_instance_id
ON job_locks(instance_id);

-- Index for finding expired locks
CREATE INDEX IF NOT EXISTS idx_job_locks_lease_expires_at
ON job_locks(lease_expires_at);

-- Index for cleanup queries
CREATE INDEX IF NOT EXISTS idx_job_locks_locked_at
ON job_locks(locked_at);
