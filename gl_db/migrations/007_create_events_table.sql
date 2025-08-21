-- Create events table for audit logging and system events
CREATE TABLE events (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT,
    entity_type TEXT, -- user, capture, job, alert, etc.
    entity_id TEXT,
    event_type TEXT NOT NULL, -- created, updated, deleted, login, logout, etc.
    details TEXT, -- JSON event details
    ip_address TEXT,
    user_agent TEXT,
    created_at TEXT NOT NULL
);

-- Indexes for event queries and auditing
CREATE INDEX idx_events_user_id ON events(user_id);
CREATE INDEX idx_events_entity ON events(entity_type, entity_id);
CREATE INDEX idx_events_type ON events(event_type);
CREATE INDEX idx_events_created_at ON events(created_at);

-- Composite index for user activity tracking
CREATE INDEX idx_events_user_activity ON events(user_id, created_at);