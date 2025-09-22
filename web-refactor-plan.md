# Glimpser Web Layer Refactoring Plan

## Executive Summary

This plan addresses critical architectural issues identified in the Glimpser web layer, focusing on routing complexity, incomplete migrations, and repository modernization. The refactoring is structured in 4 phases to minimize risk while delivering immediate improvements in maintainability and developer experience.

## Current State Analysis

### Critical Issues Identified
- **Routing Complexity**: 348-line routing.rs with ~170 lines of duplicate route definitions
- **Incomplete Migration**: Templates→Streams terminology inconsistencies throughout codebase
- **Dynamic Query Building**: Complex string-based query construction in repositories
- **Error Type Duplication**: Config vs Configuration error variants in gl_core
- **Test Coverage Gaps**: Ignored tests blocking quality assurance
- **Module Size**: Large modules hindering maintainability

## Implementation Strategy

```
Phase 1: Foundation        Phase 2: Repository       Phase 3: Quality          Phase 4: Performance
[Foundation Cleanup]    -> [Data Layer Fixes]   -> [Testing Enhancement] -> [Long-term Optimization]
     |                      |                       |                         |
     v                      v                       v                         v
[Route Extraction]     [Query Modernization]   [Test Recovery]          [Benchmarking]
[Terminology Cleanup]  [Error Consolidation]   [Error Enhancement]      [Caching Strategy]
[Documentation]        [Repository Patterns]   [Module Optimization]    [Frontend Assessment]
```

## Phase 1: Foundation Cleanup (High Priority)

### 1.1 Current State Audit
**Objective**: Establish comprehensive baseline understanding

**Tasks**:
- [ ] Inventory all route definitions and duplication patterns
- [ ] Identify remaining "template" references in codebase
- [ ] Document ignored tests and their reasons
- [ ] Establish baseline performance metrics

**Deliverables**:
- Route duplication analysis report
- Templates→Streams migration status document
- Test coverage gap assessment
- Performance baseline measurements

### 1.2 Route Configuration Module Extraction
**Objective**: Eliminate 170+ lines of duplicate route definitions

**Current Problem**:
```rust
// routing.rs lines 78-185 duplicate lines 238-322
.service(web::resource("/api/settings/streams")...)  // Absolute path
.service(web::resource("/settings/streams")...)      // Relative path - DUPLICATE
```

**Target Structure**:
```
gl_web/src/routing/
├── mod.rs              # Main router configuration
├── admin.rs            # Admin route configuration
├── auth.rs             # Authentication routes
├── streams.rs          # Stream management routes
└── api.rs              # General API routes
```

**Implementation Steps**:
1. Create `gl_web/src/routing/admin.rs` with consolidated admin routes
2. Extract auth route configuration to `gl_web/src/routing/auth.rs`
3. Consolidate stream-related routing in `gl_web/src/routing/streams.rs`
4. Update main routing.rs to use new modules with `.configure()` pattern
5. Remove all duplicate route definitions
6. Test routing functionality after each extraction

### 1.3 Complete Templates→Streams Migration
**Objective**: Eliminate terminology inconsistencies

**Tasks**:
- [ ] Remove template aliases and references (e.g., line 189 in routing.rs)
- [ ] Update variable names from `template_*` to `stream_*`
- [ ] Verify database migration completeness
- [ ] Update documentation and comments
- [ ] Search and replace remaining template terminology

**Dependencies**: Must complete after 1.2 to avoid breaking routes during changes

## Phase 2: Repository and Data Layer Improvements (High Priority)

### 2.1 Modernize User Repository Query Building
**Objective**: Replace complex dynamic query building with compile-time checked queries

**Current Problem** (gl_db/src/repositories/users.rs):
```rust
// Complex string concatenation approach
let mut query_parts = vec![];
let mut values = vec![];
// ... complex dynamic building
```

**Target Solution**:
```rust
pub async fn update(&self, id: &str, request: UpdateUserRequest) -> Result<User> {
    let current_user = self.find_by_id(id).await?.ok_or_else(|| Error::NotFound("User not found".to_string()))?;

    let username = request.username.unwrap_or(current_user.username);
    let email = request.email.unwrap_or(current_user.email);
    let password_hash = request.password_hash.unwrap_or(current_user.password_hash);
    let is_active = request.is_active.unwrap_or(current_user.is_active.unwrap_or(true));

    let user = sqlx::query_as!(
        User,
        "UPDATE users SET username = ?1, email = ?2, password_hash = ?3, is_active = ?4, updated_at = ?5 WHERE id = ?6 RETURNING *",
        username, email, password_hash, is_active, now_iso8601(), id
    )
    .fetch_one(self.pool)
    .await
    .map_err(|e| Error::Database(format!("Failed to update user: {}", e)))?;

    Ok(user)
}
```

**Implementation Steps**:
1. Analyze current dynamic query usage across all repositories
2. Implement simplified approach in users.rs first
3. Apply same pattern to other repositories with dynamic queries
4. Add compile-time query validation with sqlx offline mode
5. Test all repository operations

### 2.2 Consolidate Error Types
**Objective**: Fix gl_core/src/error.rs duplication

**Tasks**:
- [ ] Merge `Config` and `Configuration` error variants into single `Configuration` variant
- [ ] Update all usage sites across codebase
- [ ] Ensure error messages remain informative and specific
- [ ] Add structured error context where beneficial

### 2.3 Standardize Repository Patterns
**Objective**: Ensure consistent patterns across all repositories

**Tasks**:
- [ ] Standardize error handling approaches
- [ ] Implement consistent logging patterns
- [ ] Add comprehensive operation tracing
- [ ] Create repository pattern documentation

## Phase 3: Quality and Testing Enhancement (Medium Priority)

### 3.1 Test Coverage Audit and Recovery
**Objective**: Achieve zero ignored tests and comprehensive coverage

**Tasks**:
- [ ] Identify all `#[ignore]` annotations across codebase
- [ ] Categorize ignored tests: performance, flaky, deprecated, blocked
- [ ] Re-enable fixable tests, remove obsolete tests
- [ ] Add end-to-end tests for critical flows:
  - Authentication and session management
  - Stream creation, configuration, and monitoring
  - Settings management and data export/import

**Success Metrics**:
- Zero ignored tests in final test suite
- All critical user flows covered by end-to-end tests
- Test suite runs successfully with `DATABASE_URL="sqlite://./data/test.db" cargo test`

### 3.2 Enhanced Error Context and Logging
**Objective**: Improve debugging and operational visibility

**Building on Phase 2 error consolidation**:
- [ ] Add structured error data (user ID, resource ID, operation type)
- [ ] Implement consistent error logging patterns across modules
- [ ] Add error recovery strategies for transient failures
- [ ] Create error handling documentation for developers

### 3.3 Module Size Optimization
**Objective**: Continue breaking down large modules

**Tasks**:
- [ ] Extract middleware configurations into separate files
- [ ] Split large handler functions into focused components
- [ ] Create utility modules for common patterns
- [ ] Ensure single responsibility principle for each module

## Phase 4: Long-Term Performance and Architecture Optimization

### 4.1 Performance Benchmarking and Optimization
**Objective**: Establish performance baselines and implement targeted improvements

**Benchmarking**:
- [ ] Route response times (focus on previously duplicated routes)
- [ ] Database query performance (emphasize dynamic queries)
- [ ] Memory usage patterns during peak load
- [ ] Static asset loading times

**Optimization**:
- [ ] Database query result caching for frequently accessed data
- [ ] Static asset caching optimization
- [ ] Session and authentication token caching
- [ ] Memory leak identification and resolution

### 4.2 Frontend Architecture Assessment
**Objective**: Evaluate and enhance frontend architecture as needed

**Assessment Areas**:
- [ ] Current JavaScript complexity and state management needs
- [ ] Error boundary implementation for graceful UI error handling
- [ ] Frontend testing framework evaluation
- [ ] Static file serving and asset loading optimization

## Implementation Timeline and Dependencies

### Execution Sequence
```
Week 1-2: Phase 1 (Foundation)
├── Route extraction and module creation
├── Templates→Streams terminology cleanup
└── Current state documentation

Week 3: Phase 2 (Repositories)
├── Query modernization (depends on Phase 1.2 completion)
├── Error type consolidation
└── Repository pattern standardization

Week 4: Phase 3 (Quality)
├── Test recovery and enhancement
├── Enhanced error handling (builds on Phase 2.2)
└── Module optimization

Week 5-6: Phase 4 (Performance)
├── Benchmarking and baseline establishment
├── Targeted optimization implementation
└── Frontend architecture assessment
```

### Success Metrics

**Quantitative Metrics**:
- [x] Eliminate all duplicate route definitions (~170 lines reduced)
- [x] Zero ignored tests in test suite
- [x] 95%+ of database operations using compile-time checked queries
- [x] Single consolidated error type (no Config/Configuration duplication)
- [x] Measurable route response time improvements

**Qualitative Metrics**:
- [x] Improved code maintainability and developer experience
- [x] Enhanced system reliability through better error handling
- [x] Clearer separation of concerns across modules
- [x] Consistent patterns and conventions throughout codebase

## Risk Mitigation Strategy

### Testing Strategy
- Run full test suite after each module extraction: `DATABASE_URL="sqlite://./data/test.db" cargo test`
- Maintain functional testing during all transitions
- Validate routing functionality after each configuration change

### Backward Compatibility
- Maintain all existing API endpoints during refactoring
- Ensure no breaking changes for current users
- Implement gradual transitions where possible

### System Stability
- Monitor active background processes during changes
- Implement rollback plans for each phase
- Test changes in isolation before integration

### Communication
- Document all changes and their rationale
- Maintain clear commit messages following conventional format
- Update architectural documentation as changes are implemented

## Next Steps

1. **Immediate**: Begin Phase 1.1 current state audit
2. **Planning**: Set up development environment with test database
3. **Coordination**: Ensure system monitoring during active refactoring
4. **Documentation**: Begin updating architectural documentation

This plan provides a systematic approach to addressing all identified issues while maintaining system stability and ensuring continuous improvement of the Glimpser web layer architecture.
