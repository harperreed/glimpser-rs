# Glimpser Frontend Migration Plan

## Executive Summary

Migrate from vanilla HTML/CSS/JS frontend to Next.js while keeping the robust Rust backend as an API service. This hybrid architecture leverages each technology's strengths: Rust for high-performance capture/streaming, Next.js for modern UI development.

## Current State Analysis

### ✅ Rust Backend Strengths (KEEP)
- **11 clean API endpoints** with OpenAPI documentation
- High-performance capture pipeline (ffmpeg, browser automation)
- Concurrent stream processing with real-time MJPEG streaming
- Robust authentication (JWT, stateless)
- Rate limiting, body validation, structured error responses
- SQLite with complex queries and migrations
- **Zero coupling to frontend** - perfect API separation

### 🔴 Frontend Pain Points (REPLACE)
- **3,000+ lines of vanilla JS** with manual DOM manipulation
- Manual state management with global variables
- Try-catch error handling scattered everywhere
- Polling-based updates (`setInterval(loadStreams, 10000)`)
- No type safety on API responses
- Memory leak prevention with global event handlers

**Key Files:**
- `static/js/streams.js` (435 lines) - Complex stream management
- `static/js/admin.js` (668 lines) - Admin panel logic
- `static/js/dashboard.js` (232 lines) - Dashboard functionality

## Migration Strategy

### Phase 1: Foundation & API Integration (1-2 weeks)

**Objectives:**
- Set up Next.js project structure
- Generate TypeScript types from OpenAPI spec
- Implement authentication flow
- Create API client with proper error handling

**Tasks:**
- [x] Initialize Next.js project with TypeScript
- [x] Generate API types using `openapi-typescript`
- [x] Set up API client with interceptors for auth
- [x] Implement login/logout functionality
- [x] Create protected route middleware
- [x] Set up development environment with proxy to Rust backend

**Deliverables:**
- Working Next.js app with authentication
- Type-safe API client
- Login page with error handling

### Phase 2: Core Features Migration (2-3 weeks)

**Objectives:**
- Migrate stream dashboard functionality
- Replace admin panel with React components
- Implement real-time updates

**Tasks:**
- [x] Create stream dashboard page (replace `streams.js`)
  - Stream grid with filtering
  - Real-time status updates
  - Thumbnail display
  - Modal for stream details
- [x] Build admin panel (replace `admin.js`)
  - User management
  - Template management
  - API key management
- [x] Implement dashboard overview (replace `dashboard.js`)
  - System statistics
  - Recent activity
  - Quick actions

**Deliverables:**
- Fully functional stream dashboard
- Admin panel with CRUD operations
- Dashboard overview page

### Phase 3: Advanced Features & Real-time (2-4 weeks)

**Objectives:**
- Enhanced streaming integration
- Real-time notifications
- Template management UI
- Alert system dashboard

**Tasks:**
- [ ] Stream viewer with MJPEG integration
- [ ] Real-time notifications (WebSocket or SSE)
- [ ] Advanced template editor with JSON schema validation
- [ ] Alert management system
- [ ] Settings and configuration pages
- [ ] Mobile-responsive design

**Deliverables:**
- Live stream viewer
- Real-time notification system
- Complete template management
- Alert dashboard

### Phase 4: Polish & Optimization (1-2 weeks)

**Objectives:**
- Performance optimization
- Enhanced UX patterns
- Production readiness

**Tasks:**
- [ ] Image optimization for thumbnails
- [ ] Code splitting and lazy loading
- [ ] Error boundaries and loading states
- [ ] Accessibility improvements
- [ ] SEO optimization
- [ ] Production build and deployment setup

**Deliverables:**
- Production-ready Next.js application
- Optimized performance metrics
- Complete documentation

## Technical Architecture

### Backend (NO CHANGES REQUIRED)
```
Rust API Server (Port 8080)
├── Authentication (JWT)
├── Stream endpoints (/api/stream/*)
├── Admin endpoints (/api/settings/*)
├── Template management (/api/templates/*)
└── Static file serving (during transition)
```

### Frontend (NEW)
```
Next.js App (Port 3000)
├── Authentication pages
├── Dashboard
├── Stream management
├── Admin panel
├── Settings
└── API client layer
```

### Development Setup
- **Rust backend**: `cargo run` (Port 8080)
- **Next.js frontend**: `npm run dev` (Port 3000)
- **API proxy**: Next.js proxies `/api/*` to `localhost:8080`

## Migration Benefits

### Immediate Wins
- **Type Safety**: End-to-end TypeScript from API to UI
- **Developer Experience**: Hot reload, better debugging, modern tooling
- **State Management**: React state management vs manual DOM manipulation
- **Real-time Updates**: Reactive updates vs polling

### Long-term Advantages
- **Maintainability**: Component-based architecture
- **Performance**: Server-side rendering, optimized builds
- **Scalability**: Easy to add new features and team members
- **Modern UX**: Better loading states, transitions, interactions

## Risk Assessment

### Low Risk Factors ✅
- Rust backend requires zero changes (already API-ready)
- Gradual migration possible (can run both frontends temporarily)
- Well-defined API contract via OpenAPI
- No database changes required

### Considerations ⚠️
- Learning curve for team members new to React/Next.js
- Initial development time investment (8-10 weeks total)
- Need to maintain feature parity during migration

## Success Metrics

- **Development Velocity**: Faster feature development after migration
- **Bug Reduction**: Fewer frontend bugs due to type safety
- **Performance**: Better Core Web Vitals scores
- **Developer Satisfaction**: Improved development experience
- **Maintainability**: Reduced time for new feature implementation

## Timeline Summary

| Phase | Duration | Focus Area |
|-------|----------|------------|
| Phase 1 | 1-2 weeks | Foundation & Auth |
| Phase 2 | 2-3 weeks | Core Features |
| Phase 3 | 2-4 weeks | Advanced Features |
| Phase 4 | 1-2 weeks | Polish & Production |
| **Total** | **8-10 weeks** | **Complete Migration** |

## Next Steps

1. **Get approval** for migration plan
2. **Set up Next.js project** structure
3. **Generate API types** from OpenAPI spec
4. **Begin Phase 1** implementation

---

*This migration leverages our existing Rust investment while modernizing the frontend for better maintainability and developer experience.*
