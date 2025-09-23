# Glimpser Backend-Frontend Performance Optimization Plan

## Executive Summary

This plan addresses critical performance bottlenecks where backend execution is degrading frontend user experience. Based on deep analysis of the Glimpser surveillance system codebase, we've identified specific high-impact optimizations that will dramatically reduce response times and improve UX responsiveness.

**Expected Impact**: 70-90% reduction in API response times, 50% reduction in frontend blocking operations.

## 🎯 Critical Performance Bottlenecks Identified

### 1. Database Query Inefficiencies (CRITICAL)
- **Location**: `gl_web/src/routes/streams.rs:115-139`
- **Issue**: N+1 query pattern - separate queries for streams + count
- **Impact**: 2-3 DB roundtrips per request
- **Current**: `list()` + `count()` + potential user verification
- **Search broken**: No user filtering in search queries (security + performance)

### 2. Missing Cache Integration (HIGH)
- **Location**: Stream routes don't use existing cache system
- **Issue**: Sophisticated cache exists (`gl_db/src/cache.rs`) but unused
- **Impact**: Every request hits database despite 3-5min TTL cache available
- **Current**: Routes create new `StreamRepository` instances per request

### 3. Synchronous Video Processing (CRITICAL)
- **Location**: `gl_capture/src/lib.rs:133-202`
- **Issue**: `generate_snapshot_with_ffmpeg()` blocks async executor
- **Impact**: 30-second timeouts blocking request threads
- **Current**: Synchronous ffmpeg calls in async request handlers

### 4. Redundant User Verification (MEDIUM)
- **Location**: `gl_web/src/routes/streams.rs:242-265`
- **Issue**: User existence check on every stream creation
- **Impact**: Extra DB query when JWT already validated
- **Current**: Separate user lookup after authentication middleware

### 5. Job Queue Architectural Debt (LOW)
- **Location**: `gl_sched/src/job.rs:62-78`
- **Issue**: Deprecated snapshot jobs still processed
- **Impact**: Unnecessary job queue overhead
- **Current**: Returns error but still processes workflow

## 🚀 High-Impact Optimization Strategy

### Phase 1: Quick Wins (1-2 days, 40% improvement)

#### 1.1 Integrate Existing Cache Layer
```rust
// BEFORE: gl_web/src/routes/streams.rs:108
let repo = StreamRepository::new(state.db.pool());

// AFTER: Add cached repository wrapper
let cached_repo = CachedStreamRepository::new(state.db.pool(), &state.cache);
```

**Implementation Steps**:
- Create `CachedStreamRepository` wrapper in `gl_db/src/repositories/cached_streams.rs`
- Cache-first lookup for `find_by_id()`, `list()`, `count()`
- Cache invalidation on create/update/delete operations
- 5-minute TTL for stream data, 10-minute TTL for user verification

**Expected Impact**: 60-80% reduction in database queries for read operations

#### 1.2 Fix N+1 Query Pattern
```rust
// BEFORE: Separate list + count queries
let streams = repo.list(filter_user_id, offset, limit).await?;
let total = repo.count(filter_user_id).await?;

// AFTER: Single compound query
let (streams, total) = repo.list_with_total(filter_user_id, offset, limit).await?;
```

**Implementation Steps**:
- Add `list_with_total()` method using window functions
- Implement proper user filtering in search queries
- Add compound database indexes for search patterns

**Expected Impact**: 50% reduction in API response times

#### 1.3 Background Video Processing
```rust
// BEFORE: Synchronous ffmpeg in request handler
let result = generate_snapshot_with_ffmpeg(&path, &config).await?;

// AFTER: Queue job and return immediately
let job_id = snapshot_queue.enqueue(SnapshotRequest { path, config }).await?;
HttpResponse::Accepted().json(ApiResponse::success(SnapshotStatus { job_id }))
```

**Implementation Steps**:
- Move ffmpeg calls to background job queue
- Return job IDs for async status checking
- Implement WebSocket/SSE for real-time progress updates
- Add snapshot result caching

**Expected Impact**: Eliminate 30-second request timeouts

### Phase 2: Architecture Improvements (3-5 days, 30% improvement)

#### 2.1 Optimistic UI Updates with Background Sync
- Frontend updates UI immediately for user actions
- Background sync handles server-side validation
- Conflict resolution for concurrent modifications
- ETag-based optimistic concurrency (already partially implemented)

#### 2.2 Stream Aggregation Endpoints
```rust
// NEW: Single endpoint for dashboard data
GET /api/dashboard/summary -> {
    active_streams: count,
    recent_alerts: Vec<Alert>,
    system_health: Status,
    user_quota: Usage
}
```

#### 2.3 Database Query Optimization
- Add missing compound indexes identified in codebase analysis
- Implement query result caching at repository level
- Connection pool tuning for concurrent workload
- Prepared statement optimization

#### 2.4 Response Compression & Pagination
- Gzip compression for JSON responses
- Cursor-based pagination for large datasets
- Response field filtering (sparse fieldsets)
- HTTP/2 server push for critical resources

### Phase 3: Advanced Optimizations (1-2 weeks, 20% improvement)

#### 3.1 Real-time Updates Architecture
- WebSocket connections for live stream status
- Server-Sent Events for notifications
- Redis pub/sub for multi-instance deployments
- Client-side event sourcing for state management

#### 3.2 Caching Strategy Enhancement
- Multi-layer caching (in-memory + Redis)
- Cache warming strategies
- Intelligent cache invalidation patterns
- CDN integration for static assets

#### 3.3 Microservice Performance Patterns
- Request/response tracing
- Circuit breaker for external services
- Bulkhead pattern for resource isolation
- Graceful degradation modes

## 📊 Implementation Priority Matrix

| Optimization | Effort | Impact | Priority | Timeline |
|-------------|--------|---------|----------|----------|
| Cache Integration | Low | High | P0 | 1 day |
| Fix N+1 Queries | Low | High | P0 | 1 day |
| Background ffmpeg | Medium | High | P0 | 2 days |
| Stream Aggregation | Medium | Medium | P1 | 3 days |
| Real-time Updates | High | High | P1 | 1 week |
| Advanced Caching | High | Medium | P2 | 1 week |

## 🔧 Specific Code Changes Required

### 1. Database Layer (`gl_db/src/repositories/`)

#### streams.rs Enhancements
```rust
// Add compound query method
pub async fn list_with_total(
    &self,
    user_id: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Stream>, i64)> {
    // Use COUNT() OVER() window function for single query
    let query = r#"
        SELECT *, COUNT(*) OVER() as total_count
        FROM streams
        WHERE ($1::text IS NULL OR user_id = $1)
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
    "#;
    // Implementation...
}

// Add user-scoped search
pub async fn search_with_total(
    &self,
    user_id: Option<&str>,
    name_pattern: &str,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Stream>, i64)> {
    // Proper user filtering in search
}
```

#### cached_streams.rs (New File)
```rust
pub struct CachedStreamRepository<'a> {
    repo: StreamRepository<'a>,
    cache: &'a DatabaseCache,
}

impl<'a> CachedStreamRepository<'a> {
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Stream>> {
        // Check cache first, fallback to database
        if let Some(stream) = self.cache.get_stream(id) {
            return Ok(Some(stream));
        }

        let stream = self.repo.find_by_id(id).await?;
        if let Some(ref s) = stream {
            self.cache.cache_stream(s.clone());
        }
        Ok(stream)
    }
}
```

### 2. Web Layer (`gl_web/src/routes/streams.rs`)

```rust
// Replace direct repository usage with cached version
pub async fn list_streams(
    query: web::Query<ListStreamsQuery>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)?;

    // Use cached repository
    let repo = CachedStreamRepository::new(
        StreamRepository::new(state.db.pool()),
        &state.cache
    );

    // Single compound query
    let (streams, total) = if let Some(search) = &query.search {
        repo.search_with_total(Some(&user.id), search, offset, limit).await?
    } else {
        repo.list_with_total(Some(&user.id), offset, limit).await?
    };

    // Rest of implementation...
}
```

### 3. Capture Layer (`gl_capture/src/`)

#### Background Processing Service
```rust
// New: background_processor.rs
pub struct BackgroundSnapshotProcessor {
    queue: mpsc::Receiver<SnapshotRequest>,
    pool: SqlitePool,
}

impl BackgroundSnapshotProcessor {
    pub async fn process_snapshots(&mut self) {
        while let Some(request) = self.queue.recv().await {
            // Process ffmpeg in background thread
            let result = tokio::task::spawn_blocking(move || {
                // Synchronous ffmpeg call in thread pool
                generate_snapshot_sync(&request.path, &request.config)
            }).await;

            // Store result and notify completion
            self.store_snapshot_result(request.job_id, result).await;
        }
    }
}
```

### 4. New Database Indexes

```sql
-- Missing indexes for performance
CREATE INDEX IF NOT EXISTS idx_streams_user_created
ON streams(user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_streams_name_user
ON streams(name, user_id) WHERE name IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_captures_stream_created
ON captures(stream_id, created_at DESC);
```

## 🎮 Frontend Integration Strategy

### Optimistic Updates Pattern
```typescript
// Frontend: Update UI immediately, sync in background
async function updateStream(streamId: string, updates: StreamUpdate) {
    // 1. Update UI immediately
    updateStreamInUI(streamId, updates);

    // 2. Send request in background
    try {
        const response = await api.updateStream(streamId, updates);
        // 3. Confirm success or handle conflicts
        if (response.status === 409) {
            handleOptimisticUpdateConflict(streamId, response.data);
        }
    } catch (error) {
        // 4. Rollback UI changes on failure
        rollbackStreamUpdate(streamId);
        showErrorNotification(error);
    }
}
```

### Real-time Status Updates
```typescript
// WebSocket connection for live updates
const streamSocket = new WebSocket('/ws/streams');
streamSocket.onmessage = (event) => {
    const update = JSON.parse(event.data);
    updateStreamStatus(update.stream_id, update.status);
};
```

## 📈 Success Metrics & Monitoring

### Key Performance Indicators
1. **API Response Times**
   - Target: <200ms for cached reads, <500ms for writes
   - Current: 1-3 seconds for list operations

2. **Database Query Count**
   - Target: 1 query per read operation, 2 queries per write
   - Current: 2-3 queries per read, 3-5 per write

3. **Frontend Blocking Operations**
   - Target: <100ms blocking time
   - Current: 5-30 second blocks for video processing

4. **Cache Hit Rates**
   - Target: >80% for stream reads, >90% for user auth
   - Current: 0% (cache not integrated)

### Monitoring Implementation
```rust
// Add performance metrics to existing telemetry
use tracing::{info_span, instrument};

#[instrument(skip(self))]
pub async fn list_streams_cached(&self) -> Result<Vec<Stream>> {
    let start = std::time::Instant::now();
    let cache_span = info_span!("cache_lookup");

    let result = cache_span.in_scope(|| {
        self.cache.get_streams_list()
    });

    tracing::info!(
        duration_ms = start.elapsed().as_millis(),
        cache_hit = result.is_some(),
        "stream_list_performance"
    );

    result
}
```

## 🏁 Rollout Strategy

### Week 1: Foundation (Quick Wins)
- Day 1-2: Implement cache integration
- Day 3-4: Fix N+1 query patterns
- Day 5: Background video processing
- Weekend: Performance testing & validation

### Week 2: Enhancement
- Day 1-3: Stream aggregation endpoints
- Day 4-5: Real-time updates architecture
- Weekend: Load testing & optimization

### Week 3: Polish & Advanced Features
- Day 1-3: Advanced caching strategies
- Day 4-5: Monitoring & alerting
- Weekend: Documentation & deployment

## 🔒 Risk Mitigation

### Performance Regression Prevention
- Automated performance testing in CI/CD
- Response time budgets in monitoring
- Database query analysis in development
- Load testing before production deployment

### Rollback Strategy
- Feature flags for new optimizations
- Database migration rollback scripts
- A/B testing for performance changes
- Gradual rollout with monitoring

## 💡 Future Considerations

### Scalability Enhancements
- Redis cluster for distributed caching
- Database read replicas
- CDN integration for media assets
- Horizontal scaling of background workers

### User Experience Improvements
- Progressive loading patterns
- Skeleton screens for loading states
- Prefetching based on user patterns
- Intelligent client-side caching

---

**This plan transforms Glimpser from a database-heavy synchronous system into a responsive, cache-optimized, async-first surveillance platform that provides instant feedback to users while handling heavy processing in the background.**
