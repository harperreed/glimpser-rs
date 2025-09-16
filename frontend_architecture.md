# Glimpser Frontend Architecture

## Current Architecture (HTMX + Rust Templates)

Glimpser uses a **server-rendered HTML + HTMX** architecture that provides modern interactivity while maintaining simplicity and tight integration with the Rust backend.

### Frontend Stack
- **Server-Side Rendering**: Rust templates (likely Tera/Handlebars)
- **Styling**: Tailwind CSS (via CDN)
- **Interactivity**: HTMX 1.9.10 for dynamic content updates
- **No Build Step**: Direct serving of templates and assets

### Architecture Benefits

#### ✅ Strengths of Current HTMX Approach
- **Simplicity**: No complex build pipeline or bundling
- **Performance**: Minimal JavaScript, fast page loads
- **Server Integration**: Tight coupling with Rust backend logic
- **Real-time Updates**: HTMX polling for live data (`hx-trigger="every 60s"`)
- **Progressive Enhancement**: Works without JS, enhanced with HTMX
- **Developer Velocity**: Direct template editing, instant reload

#### ✅ Rust Backend Integration
- **Single Codebase**: Frontend templates live alongside backend code
- **Type Safety**: Rust structs directly serialize to template context
- **Authentication**: Server-side session management
- **API Endpoints**: Both HTML and JSON responses from same handlers
- **Static Assets**: Direct serving from Rust server

### Current Implementation

```
gl_web/
├── templates/
│   ├── base.html          # Base layout with HTMX setup
│   ├── dashboard.html     # Dashboard with live stats
│   ├── streams_grid.html  # Stream management interface
│   └── ...
├── src/
│   ├── frontend.rs        # Template rendering logic
│   └── routes/            # API & HTML route handlers
└── static/                # CSS, JS, images
```

### Key HTMX Features Used

- **Auto-refresh**: `hx-trigger="load, every 60s"` for live dashboard
- **Loading States**: HTMX indicators with CSS transitions
- **Swapping**: `hx-swap="innerHTML"` for partial page updates
- **Progressive Enhancement**: Graceful degradation without JS

## Why Not React/Next.js?

The **HTMX approach is superior** for this use case:

1. **Simpler Deployment**: Single binary with embedded templates
2. **Better Performance**: No client-side hydration or bundle size
3. **Easier Maintenance**: No separate frontend/backend coordination
4. **Type Safety**: Rust types flow directly to templates
5. **Real-time**: HTMX polling is simpler than WebSocket setup

## Development Workflow

1. **Backend Changes**: Modify Rust handlers and restart server
2. **Frontend Changes**: Edit templates, refresh browser
3. **Styling**: Update Tailwind classes directly in templates
4. **New Features**: Add route handlers + corresponding templates

## Recommended Enhancements

Rather than migrating away from HTMX, focus on:

- **Enhanced HTMX Patterns**: More sophisticated interactions
- **Better Error Handling**: HTMX error responses and user feedback
- **Real-time Features**: WebSocket integration with HTMX
- **Component System**: Reusable template partials
- **Advanced Styling**: Custom CSS for complex interactions

---

*The HTMX + Rust architecture provides the perfect balance of modern UX and architectural simplicity for Glimpser's streaming management needs.*
