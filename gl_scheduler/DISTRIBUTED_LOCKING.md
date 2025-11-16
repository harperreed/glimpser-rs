# Distributed Locking for Job Execution

## Overview

The job scheduler now includes **distributed locking** to prevent duplicate job execution when multiple application instances are running concurrently. This addresses Issue #16 by ensuring that only one instance can execute a job at any given time.

## Problem Statement

Without distributed locking, when multiple instances of the application run:
- Each instance loads all persisted jobs from the database
- Multiple instances attempt to execute the same job simultaneously
- This leads to:
  - **Duplicate job execution** (same job runs multiple times)
  - **Data corruption** from concurrent writes
  - **Wasted resources** running duplicate work

## Solution: Database-Based Distributed Locking

### Architecture

The implementation uses **database-based distributed locking** with the following components:

1. **Job Locks Table** (`job_locks`)
   - Tracks which instance holds a lock on which job
   - Stores lock metadata (acquisition time, expiration, status)

2. **Instance Identification**
   - Each application instance has a unique ID: `{hostname}:{process_id}`
   - Example: `web-server-1:12345`

3. **Lease-Based Locking**
   - Locks have an expiration time (default: 6 minutes)
   - Prevents deadlocks from crashed instances
   - Allows lock takeover after expiration

4. **Idempotency**
   - Each job execution has a unique UUID (`execution_id`)
   - Tracked in `job_executions` table
   - Prevents duplicate processing even if a job is accidentally run twice

### How It Works

#### Lock Acquisition Flow

```
Instance A                    Database                    Instance B
    |                            |                            |
    |--[Try to lock job-123]---->|                            |
    |                            |                            |
    |<---[Lock acquired]---------|                            |
    |                            |                            |
    |                            |<--[Try to lock job-123]---|
    |                            |                            |
    |                            |---[Lock conflict]--------->|
    |                            |                            |
    |--[Execute job]             |                            |
    |                            |                         [Skip]
    |--[Release lock]---------->|                            |
    |                            |                            |
```

#### Lock Expiration & Takeover

```
Instance A (crashes)          Database                    Instance B
    |                            |                            |
    |--[Lock acquired]---------->|                            |
    |                            |                            |
    X  [CRASH!]                  |                            |
                                 |                            |
                                 |                            |
                         [Time passes...]                     |
                                 |                            |
                                 |<--[Try to lock job-123]---|
                                 |                            |
                                 |   [Check expiration]       |
                                 |   [Lock expired!]          |
                                 |                            |
                                 |---[Lock acquired]--------->|
                                 |                            |
                                 |                        [Execute]
```

## Configuration

### Enable/Disable Distributed Locking

```rust
use gl_scheduler::SchedulerConfig;

let config = SchedulerConfig {
    enable_distributed_locking: true,  // Enable (default: true)
    lock_lease_seconds: 360,            // 6 minutes (default)
    ..Default::default()
};
```

**⚠️ WARNING**: Disabling distributed locking is **not safe** for multi-instance deployments!

### Configuration Options

| Option | Default | Description |
|--------|---------|-------------|
| `enable_distributed_locking` | `true` | Enable/disable distributed locking |
| `lock_lease_seconds` | `360` | Lock expiration time (should be > job timeout) |
| `history_retention_days` | `30` | How long to keep old lock records |

## Database Schema

### job_locks Table

```sql
CREATE TABLE job_locks (
    id TEXT PRIMARY KEY,                -- Lock ID (UUID)
    job_id TEXT NOT NULL,               -- Job being locked
    execution_id TEXT NOT NULL,         -- Execution attempt ID
    instance_id TEXT NOT NULL,          -- Instance holding lock
    locked_at TEXT NOT NULL,            -- When lock was acquired
    lease_expires_at TEXT NOT NULL,     -- When lock expires
    status TEXT NOT NULL,               -- acquired, released, expired
    released_at TEXT,                   -- When lock was released
    FOREIGN KEY (job_id) REFERENCES scheduled_jobs(id)
);
```

## API Usage

### Get Lock Statistics

```rust
let scheduler = JobScheduler::new(config, storage, db, capture_service).await?;

// Get lock statistics
if let Some(stats) = scheduler.get_lock_stats().await? {
    println!("Acquired locks: {}", stats.acquired_count);
    println!("Released locks: {}", stats.released_count);
    println!("Expired locks: {}", stats.expired_count);
}
```

### Get Instance ID

```rust
// Useful for debugging multi-instance deployments
if let Some(instance_id) = scheduler.get_instance_id() {
    println!("Running on instance: {}", instance_id);
}
```

## Lock Lifecycle

### 1. Lock Acquisition
- Before executing a job, the scheduler attempts to acquire a lock
- If successful, proceeds with execution
- If lock is held by another instance, skips execution

### 2. Job Execution
- Lock is held for the duration of job execution
- Lock lease prevents indefinite holds (handles crashes)

### 3. Lock Release
- Upon job completion (success or failure), lock is released
- Lock can be acquired by other instances

### 4. Lock Cleanup
- **Stale Lock Expiration**: Runs every minute, marks expired locks
- **Old Lock Cleanup**: Runs every hour, deletes old released/expired locks
- Retention period: `history_retention_days` configuration

## Multi-Instance Deployment

### Docker Compose Example

```yaml
services:
  glimpser-instance-1:
    image: glimpser:latest
    environment:
      - DATABASE_URL=/app/data/glimpser.db
      - ENABLE_DISTRIBUTED_LOCKING=true
    volumes:
      - shared-data:/app/data

  glimpser-instance-2:
    image: glimpser:latest
    environment:
      - DATABASE_URL=/app/data/glimpser.db
      - ENABLE_DISTRIBUTED_LOCKING=true
    volumes:
      - shared-data:/app/data

volumes:
  shared-data:
```

**Important**: All instances must:
- Share the same database
- Have distributed locking enabled
- Have unique hostnames or process IDs

### Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: glimpser
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: glimpser
        image: glimpser:latest
        env:
        - name: DATABASE_URL
          value: "sqlite:///shared/glimpser.db"
        - name: ENABLE_DISTRIBUTED_LOCKING
          value: "true"
        volumeMounts:
        - name: shared-storage
          mountPath: /shared
```

## Monitoring & Debugging

### Check Lock Status

Query the database to see active locks:

```sql
-- See all active locks
SELECT job_id, instance_id, locked_at, lease_expires_at
FROM job_locks
WHERE status = 'acquired';

-- See which instance is executing which job
SELECT j.name, l.instance_id, l.locked_at
FROM job_locks l
JOIN scheduled_jobs j ON l.job_id = j.id
WHERE l.status = 'acquired';

-- Find expired locks that should be cleaned up
SELECT * FROM job_locks
WHERE status = 'acquired'
  AND lease_expires_at < datetime('now');
```

### Logs

The scheduler logs lock-related events:

```
INFO  Distributed locking enabled for instance: web-server-1:12345
INFO  Acquired lock for job snapshot-job (execution abc123)
INFO  Job snapshot-job is already locked by web-server-2:67890, skipping execution
INFO  Released lock for job snapshot-job
WARN  Found expired lock for job snapshot-job, allowing takeover
```

## Testing

The distributed lock manager includes comprehensive tests:

```bash
cd gl_scheduler
cargo test distributed_lock
```

Test coverage includes:
- Lock acquisition and release
- Preventing duplicate execution
- Expired lock takeover
- Lock cleanup
- Lock statistics

## Performance Considerations

### Database Impact
- Each job execution requires:
  - 1 INSERT (lock acquisition)
  - 1 UPDATE (lock release)
  - Periodic SELECT queries (stale lock expiration)

### Optimizations
- Indexes on `job_id`, `status`, and `lease_expires_at`
- Batch cleanup operations
- Efficient lock expiration queries

### Scalability
- Tested with SQLite (suitable for small-medium deployments)
- For high-scale deployments, consider:
  - PostgreSQL with advisory locks
  - Redis-based locking
  - Dedicated lock coordination service (e.g., etcd, Consul)

## Failure Scenarios

### Instance Crashes
- **Problem**: Instance crashes while holding a lock
- **Solution**: Lock lease expires, another instance can take over
- **Recovery**: Automatic via lock expiration mechanism

### Network Partitions
- **Problem**: Instance loses database connectivity
- **Solution**: SQLite timeout (30 seconds), lock expiration
- **Recovery**: Instance reconnects or lock expires

### Database Unavailable
- **Problem**: Database is temporarily unavailable
- **Solution**: Job execution fails, retries on next schedule
- **Recovery**: Automatic when database is restored

## Migration from Non-Distributed Setup

1. **Run the migration**:
   ```bash
   # Migration 025 creates the job_locks table
   # This is automatically applied on application startup
   ```

2. **Update configuration**:
   ```rust
   let config = SchedulerConfig {
       enable_distributed_locking: true,
       ..Default::default()
   };
   ```

3. **Deploy instances**:
   - Deploy multiple instances with shared database
   - Each instance automatically coordinates via locks

4. **Monitor**:
   - Check logs for lock acquisition/release
   - Query `job_locks` table to verify correct operation

## Best Practices

1. **Always enable distributed locking** for multi-instance deployments
2. **Set lock lease > job timeout** to prevent premature expiration
3. **Monitor lock statistics** to detect issues
4. **Use unique hostnames** for each instance
5. **Share the same database** across all instances
6. **Set appropriate retention** for lock history

## Troubleshooting

### Jobs Not Executing
- **Check**: Lock status in `job_locks` table
- **Check**: Instance IDs are unique
- **Check**: Database connectivity from all instances

### Duplicate Execution
- **Check**: Distributed locking is enabled
- **Check**: All instances use the same database
- **Check**: Lock lease duration is appropriate

### Lock Contention
- **Check**: Multiple instances trying to execute same job
- **Solution**: This is expected behavior - only one succeeds

### Stale Locks
- **Check**: Lock cleanup task is running
- **Check**: `lease_expires_at` values
- **Manual cleanup**: Update expired locks to `status = 'expired'`

## Future Enhancements

Potential improvements for future versions:

1. **Redis-based locking** for better performance at scale
2. **Lock priority** for critical jobs
3. **Lock wait/retry** instead of immediate skip
4. **Distributed tracing** for lock operations
5. **Web UI** for lock monitoring
6. **Metrics/Prometheus** integration for lock statistics

## References

- Issue #16: No Distributed Locking for Job Execution
- SQLite Locking: https://www.sqlite.org/lockingv3.html
- Distributed Systems Patterns: https://martinfowler.com/articles/patterns-of-distributed-systems/
