# COMPLETE NEXT.JS → AXUM + HTMX + ASKAMA REPLACEMENT PLAN

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
│  (Axum + HTMX + Askama)        │
│                                 │
│  /login     → Server-rendered   │
│  /dashboard → HTMX + Askama     │
│  /streams   → Real-time HTMX    │
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
- Transform current `gl_web` from pure Actix-web API → Axum + HTMX + Askama hybrid
- Keep existing gl_db, gl_capture, etc. integrations
- Add axum, htmx, and askama dependencies to existing gl_web crate

**2. Development Environment**
```
gl_web:  Port 3000 (Unified: Server-rendered pages + minimal API)
```

**3. Core Dependencies Setup**
```toml
# Updated gl_web/Cargo.toml (add to existing dependencies)
axum = "0.7"
askama = { version = "0.12", features = ["with-axum"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["fs", "trace"] }
# Keep existing: actix-web, gl_db, gl_capture, etc.
# HTMX served as static asset from CDN or local
```

## PHASE 2: CORE INFRASTRUCTURE

### Week 3-4: Essential Systems

**Authentication System**
- Design: JWT tokens managed within unified gl_web service
- Storage: HTTP-only cookies for browser sessions
- Validation: Axum middleware for server-rendered and API routes

**Asset Pipeline**
- Tailwind CSS compilation via external process or CDN
- Static asset serving (HTMX, CSS, images) through tower-http
- Hot-reload development setup with cargo-watch

**Template + HTMX Integration**
```rust
// Askama templates with HTMX for dynamic updates
use askama::Template;
use gl_db::repositories::StreamRepository;

#[derive(Template)]
#[template(path = "streams.html")]
struct StreamsTemplate {
    streams: Vec<Stream>,
    user: User,
}

async fn get_streams(user: User) -> Result<Html<String>, Error> {
    let streams = StreamRepository::list_active().await?;
    let template = StreamsTemplate { streams, user };
    Ok(Html(template.render()?))
}
```

## PHASE 3: COMPLETE UI IMPLEMENTATION

### Week 5-8: All Pages Development

**Component Mapping Strategy:**
```
React Components     → Askama Templates + HTMX
useState/useEffect   → Server-side state + HTMX updates
API calls           → Direct database/service calls
Form validation     → Server-side form handling + HTMX
Error boundaries    → Axum error handlers
Real-time polling   → HTMX polling or Server-Sent Events
```

**Page Implementation Priority:**
1. **Login Page** - Form handling, authentication flow
2. **Dashboard** - Data display, basic live updates
3. **Streams List** - Real-time updates, filtering
4. **Individual Stream** - Complex state, live controls
5. **Admin Interface** - CRUD operations, management

**Askama Template Patterns:**
```rust
#[derive(Template)]
#[template(path = "stream_detail.html")]
struct StreamDetailTemplate {
    stream: StreamInfo,
    user: UserInfo,
    is_live: bool,
    recent_events: Vec<StreamEvent>,
}

// HTMX endpoint for real-time updates
async fn stream_status_fragment(
    Path(stream_id): Path<String>
) -> Result<Html<String>, Error> {
    let stream = StreamRepository::get(stream_id).await?;
    let template = StreamStatusFragment {
        is_live: stream.status == StreamStatus::Live,
        viewer_count: stream.viewer_count
    };
    Ok(Html(template.render()?))
}
```

## PHASE 4: TESTING & DEPLOYMENT

### Week 9-10: Switch Execution

**Comprehensive Testing**
- Feature parity checklist against current Next.js app
- Performance benchmarking vs React frontend
- Real-time functionality validation under load

**Big Bang Deployment**
- Switch from Next.js frontend to unified gl_web with HTMX
- Complete removal of Next.js frontend
- Single service deployment and monitoring

## TECHNICAL IMPLEMENTATION DETAILS

### Real-time Features Migration
```
Current: React polling API endpoints
New:     HTMX polling + Server-Sent Events
Pattern: Server-rendered HTML fragments via HTMX
```

### State Management Strategy
```
Current: React Context + useState
New:     Server-side state + Askama templates
Sync:    HTMX requests for dynamic updates
```

### Asset Handling
```
CSS:     Tailwind → External compilation or CDN
Static:  Tower-http static file serving (HTMX, CSS, images)
JS:      HTMX from CDN or local static files
Dev:     Cargo-watch for Rust hot reload
```

## RISK MITIGATION

**High Priority Risks:**
- **HTMX Learning Curve** - Team needs to learn HTMX patterns and best practices
- **Performance** - Benchmark server-side rendering vs client-side React
- **Real-time Updates** - Test HTMX polling performance under realistic load

**Backup Strategy:**
- Keep Next.js code in git branch until new system proven
- Feature validation checklist for complete parity
- Performance baseline measurements for comparison

## SUCCESS CRITERIA

**Week 1 Go/No-Go Evaluation:**
- [ ] HTML pages render from unified gl_web service
- [ ] Form submissions work with HTMX + Askama
- [ ] Authentication flows within single service
- [ ] Tailwind CSS compilation functional

**Final Success Metrics:**
- [ ] Complete feature parity with React frontend
- [ ] Real-time updates working with HTMX
- [ ] Performance meets or exceeds current system
- [ ] Clean unified service architecture

## IMMEDIATE NEXT STEPS

**Day 1:** Research axum + HTMX + askama capabilities and examples ✅
**Day 2:** Add axum + askama dependencies to gl_web
**Day 3:** Set up hybrid routing (server-rendered + existing API endpoints)
**Day 4-5:** Build login page proof-of-concept

## NOTES

Ready to start implementation! This unified approach is beautifully simple:
- One service (gl_web)
- One port (3000)
- Server-rendered pages with Askama templates + HTMX interactivity
- Direct database access from route handlers
- Minimal API endpoints only for external integrations
- Production-ready stack with proven libraries
- All the power of Rust throughout the stack

**Key Advantages of HTMX + Askama Approach:**
- **Production Ready**: Mature libraries with active communities
- **Simple Mental Model**: Server renders HTML, HTMX adds interactivity
- **Fast First Paint**: Server-side rendering for excellent SEO and performance
- **Type Safety**: Compile-time template checking with Askama
- **Minimal JavaScript**: HTMX handles DOM updates, no complex client-side state
