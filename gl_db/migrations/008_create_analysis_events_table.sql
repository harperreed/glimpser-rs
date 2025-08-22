-- Create analysis_events table for motion detection, AI analysis, and rule engine events
CREATE TABLE analysis_events (
    id TEXT PRIMARY KEY NOT NULL,
    template_id TEXT NOT NULL,
    event_type TEXT NOT NULL, -- motion_detected, person_detected, fire_alert, etc.
    severity TEXT NOT NULL, -- info, low, medium, high, critical
    confidence REAL NOT NULL, -- 0.0 to 1.0
    description TEXT NOT NULL,
    metadata TEXT, -- JSON structured metadata
    processor_name TEXT NOT NULL, -- motion, ai_description, summary, etc.
    source_id TEXT NOT NULL, -- camera, sensor, etc.
    should_notify BOOLEAN NOT NULL DEFAULT false,
    suggested_actions TEXT, -- JSON array of suggested actions
    created_at TEXT NOT NULL,
    FOREIGN KEY (template_id) REFERENCES templates(id) ON DELETE CASCADE
);

-- Indexes for analysis event queries
CREATE INDEX idx_analysis_events_template_id ON analysis_events(template_id);
CREATE INDEX idx_analysis_events_type ON analysis_events(event_type);
CREATE INDEX idx_analysis_events_severity ON analysis_events(severity);
CREATE INDEX idx_analysis_events_should_notify ON analysis_events(should_notify) WHERE should_notify = true;
CREATE INDEX idx_analysis_events_created_at ON analysis_events(created_at);
CREATE INDEX idx_analysis_events_source ON analysis_events(source_id);

-- Composite index for notification queries
CREATE INDEX idx_analysis_events_notifications ON analysis_events(should_notify, severity, created_at);
