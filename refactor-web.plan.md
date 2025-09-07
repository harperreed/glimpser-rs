# COMPLETE NEXT.JS → AXUM-LIVE-VIEW REPLACEMENT PLAN

## ARCHITECTURE OVERVIEW

```
CURRENT STATE:
┌─────────────────┐    ┌──────────────────┐
│   Next.js       │───▶│    gl_web        │
│   Frontend      │    │   (REST API)     │
│   (React/TS)    │    │   (Actix-web)    │
└─────────────────┘    └──────────────────┘

NEW UNIFIED STATE:
┌─────────────────────────────────┐
│           gl_web                │
│    (Axum + Live-View)          │
│                                 │
│  /login     → Live-View Page    │
│  /dashboard → Live-View Page    │
│  /streams   → Live-View Page    │
│  /api/upload → REST endpoint    │
│  /api/webhook → REST endpoint   │
└─────────────────────────────────┘
         │
         ▼
   ┌──────────────┐
   │   Database   │
   └──────────────┘
```

## PHASE 1: FOUNDATION & SETUP

### Week 1-2: Infrastructure Setup

**1. Service Transformation**
- Transform current `gl_web` from pure Actix-web API → Axum + Live-View hybrid
- Keep existing gl_db, gl_capture, etc. integrations
- Add axum-live-view dependencies to existing gl_web crate

**2. Development Environment**
```
gl_web:  Port 3000 (Unified: Live-View pages + minimal API)
```

**3. Core Dependencies Setup**
```toml
# Updated gl_web/Cargo.toml (add to existing dependencies)
axum = "0.7"
axum-live-view = "0.1"
tower = "0.4"
tower-http = { version = "0.5", features = ["fs"] }
# Keep existing: actix-web, gl_db, gl_capture, etc.
```

## PHASE 2: CORE INFRASTRUCTURE

### Week 3-4: Essential Systems

**Authentication System**
- Design: JWT tokens managed within unified gl_web service
- Storage: HTTP-only cookies for browser sessions
- Validation: Axum middleware for live-view and API routes

**Asset Pipeline**
- Tailwind CSS compilation via trunk or direct CSS compilation
- Static asset serving through tower-http
- Hot-reload development setup

**Direct Database Access**
```rust
// Live-view components directly access gl_db
use gl_db::repositories::StreamRepository;

impl LiveView for StreamPage {
    async fn mount(&mut self, ctx: &LiveViewContext) {
        let streams = StreamRepository::list_active().await?;
        self.update_streams(streams);
    }
}
```

## PHASE 3: COMPLETE UI IMPLEMENTATION

### Week 5-8: All Pages Development

**Component Mapping Strategy:**
```
React Components     → Live-View Components
useState/useEffect   → Server-side state management
API calls           → Direct database/service calls
Form validation     → Server-side form handling
Error boundaries    → Axum error handlers
Real-time polling   → WebSocket push events
```

**Page Implementation Priority:**
1. **Login Page** - Form handling, authentication flow
2. **Dashboard** - Data display, basic live updates
3. **Streams List** - Real-time updates, filtering
4. **Individual Stream** - Complex state, live controls
5. **Admin Interface** - CRUD operations, management

**Live-View State Patterns:**
```rust
#[derive(Clone)]
struct StreamPageState {
    stream_id: String,
    stream_data: StreamInfo,
    user_session: UserInfo,
    live_status: StreamStatus,
    // Direct access to repositories
    stream_repo: Arc<StreamRepository>,
    user_repo: Arc<UserRepository>,
}
```

## PHASE 4: TESTING & DEPLOYMENT

### Week 9-10: Switch Execution

**Comprehensive Testing**
- Feature parity checklist against current Next.js app
- Performance benchmarking vs React frontend
- Real-time functionality validation under load

**Big Bang Deployment**
- Switch from Next.js frontend to unified gl_web live-view
- Complete removal of Next.js frontend
- Single service deployment and monitoring

## TECHNICAL IMPLEMENTATION DETAILS

### Real-time Features Migration
```
Current: React polling API endpoints
New:     Live-view push events directly from services
Pattern: Event broadcasting via internal channels
```

### State Management Strategy
```
Current: React Context + useState
New:     Server-side Live-view state with direct DB access
Sync:    Selective client updates via WebSocket
```

### Asset Handling
```
CSS:     Tailwind → Rust compilation pipeline
Static:  Tower-http static file serving
Dev:     Tower-livereload for hot updates
```

## RISK MITIGATION

**High Priority Risks:**
- **Axum-Live-View Maturity** - Build proof-of-concept for complex pages first
- **Performance** - Benchmark server-side rendering vs client-side React
- **Real-time Updates** - Test WebSocket handling under realistic load

**Backup Strategy:**
- Keep Next.js code in git branch until new system proven
- Feature validation checklist for complete parity
- Performance baseline measurements for comparison

## SUCCESS CRITERIA

**Week 1 Go/No-Go Evaluation:**
- [ ] HTML pages render from new gl_web service
- [ ] Form submissions work with live-view
- [ ] Authentication flows against gl_api backend
- [ ] Tailwind CSS compilation functional

**Final Success Metrics:**
- [ ] Complete feature parity with React frontend
- [ ] Real-time updates working smoothly
- [ ] Performance meets or exceeds current system
- [ ] Clean service separation (gl_web ↔ gl_api)

## IMMEDIATE NEXT STEPS

**Day 1:** Research axum-live-view capabilities and examples
**Day 2:** Add axum + axum-live-view dependencies to gl_web
**Day 3:** Set up hybrid routing (live-view + existing API endpoints)
**Day 4-5:** Build login page proof-of-concept

## NOTES

Ready to start implementation! This unified approach is beautifully simple:
- One service (gl_web)
- One port (3000)
- Live-view pages with direct database access
- Minimal API endpoints only for external integrations
- All the power of Rust throughout the stack
