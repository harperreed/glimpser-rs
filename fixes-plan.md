# Glimpser Platform Implementation Fixes Plan

**Date**: 2025-08-31
**Status**: CRITICAL - System Architecture Complete, Execution Layer Missing
**Priority**: URGENT - 70% implementation gap identified

## Executive Summary

Glimpser has excellent Rust architecture with enterprise-grade foundations, but there's a massive execution gap between the designed system (173+ endpoints, full surveillance platform) and current reality (~25 working endpoints, no active streaming). The frontend is production-ready but receives empty data from placeholder backend endpoints.

**Core Problem**: Templates (configurations) exist but cannot be executed as running streams. No capture pipeline connects templates to live data.

## Critical Issues Analysis

### 1. **Template ↔ Stream Disconnect**
- ✅ Templates stored properly in database (website capture configs)
- ❌ No execution system to run templates as active streams
- ❌ Frontend expects live stream data that doesn't exist
- ❌ `/api/streams` returns `[]` instead of template-based stream data

### 2. **Broken Core Functionality**
- ⚠️ **Stream endpoints** (routing middleware conflicts preventing access)
- ❌ **MJPEG streaming** (returns 501 Not Implemented)
- ❌ **Template execution** (no job scheduling/capture pipeline)
- ❌ **Real-time data** (no active monitoring/status updates)

### 3. **Frontend-Backend Contract Mismatch**
```javascript
// Frontend expects:
GET /api/streams → [{id, name, status, source, resolution, fps, last_frame_at}]
GET /api/streams/{id} → {stream details}
POST /api/streams/{id}/start → {success}
DELETE /api/streams/{id}/stop → {success}

// Backend currently provides:
GET /api/streams → [] (empty placeholder)
GET /api/stream/{id}/snapshot → 404 (routing broken)
GET /api/stream/{id}/mjpeg → 501 Not Implemented
```

## Implementation Roadmap

### Phase 1: Core Streaming Foundation (Week 1)
**Priority: CRITICAL - Must complete to have basic functionality**

#### Day 1-2: Fix Stream Route Access
- [ ] **Fix middleware routing conflict** on `/api/stream/*` endpoints
  - Remove problematic rate limiting + auth middleware combination
  - Test `/api/stream/{id}/snapshot` endpoint accessibility
  - Verify template lookup and basic capture flow works

#### Day 3-4: Implement Active Streams API
- [ ] **Replace placeholder `/api/streams` endpoint**
  ```rust
  // Transform templates into "active streams" for frontend
  pub async fn streams(state: web::Data<AppState>) -> Result<HttpResponse> {
      let templates = TemplateRepository::new(state.db.pool()).list_all().await?;
      let streams: Vec<StreamInfo> = templates.into_iter().map(|template| {
          StreamInfo {
              id: template.id.clone(),
              name: template.name,
              source: extract_source_from_config(&template.config),
              status: "active", // TODO: Check actual execution status
              resolution: "1920x1080", // Extract from template config
              fps: 1, // Website templates = ~1fps snapshots
              last_frame_at: Some(chrono::Utc::now()), // TODO: Real timestamp
          }
      }).collect();
      Ok(HttpResponse::Ok().json(streams))
  }
  ```

#### Day 5-7: Template-to-Stream Mapping
- [ ] **Create StreamInfo model** matching frontend expectations
- [ ] **Add individual stream endpoints** (`/api/streams/{id}`)
- [ ] **Map template configs** to stream metadata (resolution, source URL, etc.)
- [ ] **Test frontend-backend integration** (dashboard should show templates as streams)

### Phase 2: Template Execution System (Week 2)
**Priority: HIGH - Enables actual capture functionality**

#### Day 8-10: Basic Capture Pipeline
- [ ] **Create CaptureManager service**
  ```rust
  pub struct CaptureManager {
      active_captures: HashMap<String, CaptureHandle>,
      db_pool: Arc<SqlitePool>,
  }

  impl CaptureManager {
      pub async fn start_template(&self, template_id: &str) -> Result<()>
      pub async fn stop_template(&self, template_id: &str) -> Result<()>
      pub async fn get_status(&self, template_id: &str) -> CaptureStatus
  }
  ```

#### Day 11-12: Website Template Execution
- [ ] **Implement website capture** using existing gl_capture::WebsiteSource
- [ ] **Schedule periodic snapshots** (based on template config)
- [ ] **Store capture results** in database/filesystem
- [ ] **Update stream status** with real execution state

#### Day 13-14: Stream Lifecycle Management
- [ ] **Add start/stop stream endpoints**
  - `POST /api/streams/{id}/start`
  - `POST /api/streams/{id}/stop`
- [ ] **Integrate with CaptureManager**
- [ ] **Stream status tracking** (active/inactive/error states)
- [ ] **Frontend stream controls** (start/stop buttons)

### Phase 3: Real-time Streaming (Week 3)
**Priority: MEDIUM - Enhances user experience**

#### Day 15-17: MJPEG Stream Implementation
- [ ] **Replace 501 Not Implemented** with basic JPEG-over-HTTP
- [ ] **Serve latest snapshots** from capture pipeline
- [ ] **Basic MJPEG multipart streaming**
- [ ] **Rate limiting for stream endpoints** (prevent DoS)

#### Day 18-19: Real-time Dashboard Updates
- [ ] **WebSocket/SSE endpoint** for live status updates
- [ ] **Stream health monitoring**
- [ ] **Frontend auto-refresh** of stream status
- [ ] **Error state handling** and user notifications

#### Day 20-21: Alert System Integration
- [ ] **Connect captures to alert system**
- [ ] **Motion detection triggers** (if AI analysis enabled)
- [ ] **Alert dispatch pipeline** (email/SMS/webhooks)
- [ ] **Alert history in dashboard**

## Technical Implementation Details

### Database Schema Updates Needed
```sql
-- Add stream execution tracking
ALTER TABLE templates ADD COLUMN last_executed_at DATETIME;
ALTER TABLE templates ADD COLUMN execution_status VARCHAR(20) DEFAULT 'stopped';
ALTER TABLE templates ADD COLUMN last_error_message TEXT;

-- Track capture results
CREATE TABLE captures (
    id TEXT PRIMARY KEY,
    template_id TEXT NOT NULL,
    captured_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    file_path TEXT,
    status VARCHAR(20),
    error_message TEXT,
    FOREIGN KEY (template_id) REFERENCES templates(id)
);
```

### Key Components to Implement

#### 1. StreamInfo Model
```rust
#[derive(Serialize, Deserialize)]
pub struct StreamInfo {
    pub id: String,
    pub name: String,
    pub source: String,
    pub status: StreamStatus, // active, inactive, error
    pub resolution: String,
    pub fps: u32,
    pub last_frame_at: Option<DateTime<Utc>>,
    pub template_id: String,
}

#[derive(Serialize, Deserialize)]
pub enum StreamStatus {
    Active,
    Inactive,
    Error(String),
    Starting,
    Stopping,
}
```

#### 2. CaptureManager Integration
```rust
// In main.rs application startup
let capture_manager = Arc::new(CaptureManager::new(db.clone()));
let app_state = AppState {
    db,
    jwt_secret,
    capture_manager: capture_manager.clone(),
    // ... other fields
};

// Background task for executing active templates
tokio::spawn(async move {
    capture_manager.run_scheduler().await;
});
```

#### 3. Stream Route Fixes
```rust
// Fix middleware ordering in lib.rs
.service(
    web::scope("/stream")
        .wrap(middleware::auth::RequireAuth::new()) // Auth first
        // Remove problematic rate limiting for now
        .service(stream::snapshot)
        .service(stream::mjpeg_stream)
)
```

## Success Criteria

### Week 1 Deliverables
- [ ] Stream endpoints accessible (no more 404s)
- [ ] Dashboard shows templates as "streams"
- [ ] Frontend-backend data contract working

### Week 2 Deliverables
- [ ] Templates can be started/stopped as streams
- [ ] Basic website capture working
- [ ] Stream status updates in real-time

### Week 3 Deliverables
- [ ] MJPEG streaming functional
- [ ] Alert system connected
- [ ] Full surveillance platform operational

## Risk Assessment

### High Risk Items
1. **Template execution complexity** - Website capture may have browser/timing issues
2. **Performance under load** - Multiple concurrent streams could overwhelm system
3. **Frontend state management** - Real-time updates may cause UI flickering

### Mitigation Strategies
1. **Graceful error handling** - Capture failures shouldn't crash streams
2. **Resource limits** - Max concurrent streams configuration
3. **Progressive enhancement** - Basic functionality first, real-time second

## Resource Requirements

### Development Time
- **Phase 1**: 40 hours (1 week focused development)
- **Phase 2**: 40 hours (1 week focused development)
- **Phase 3**: 40 hours (1 week focused development)
- **Total**: ~120 hours over 3 weeks

### Infrastructure Needs
- **Existing**: Database, web server, authentication - all functional
- **Missing**: Background job processing, file storage management
- **Optional**: Redis for session/status caching, WebSocket infrastructure

## Conclusion

The Glimpser platform has excellent architectural foundations but needs the execution layer implemented. With focused development following this plan, we can transform it from a sophisticated but non-functional system into a working enterprise surveillance platform.

The key insight is that **templates are not streams** - they are configurations that need to be executed as running processes to become streams. Once this execution layer is built, the existing frontend will immediately become functional.

**Next Step**: Begin Phase 1 - Fix stream route access and implement the `/api/streams` endpoint transformation.
