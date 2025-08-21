-- Create alerts table for system notifications and user alerts  
CREATE TABLE alerts (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    capture_id TEXT,
    alert_type TEXT NOT NULL, -- motion_detected, error, system, user_defined, etc.
    severity TEXT NOT NULL DEFAULT 'info', -- info, warning, error, critical
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    metadata TEXT, -- JSON alert metadata
    is_read BOOLEAN NOT NULL DEFAULT false,
    is_dismissed BOOLEAN NOT NULL DEFAULT false,
    triggered_at TEXT NOT NULL,
    read_at TEXT,
    dismissed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE
);

-- Indexes for alert queries
CREATE INDEX idx_alerts_user_id ON alerts(user_id);
CREATE INDEX idx_alerts_unread ON alerts(user_id, is_read) WHERE is_read = false;
CREATE INDEX idx_alerts_severity ON alerts(severity);
CREATE INDEX idx_alerts_type ON alerts(alert_type);
CREATE INDEX idx_alerts_triggered_at ON alerts(triggered_at);