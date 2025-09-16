-- Create notification_deliveries table for tracking alert delivery status per channel
CREATE TABLE notification_deliveries (
    id TEXT PRIMARY KEY NOT NULL,
    analysis_event_id TEXT NOT NULL,
    channel_type TEXT NOT NULL, -- pushover, webhook, email, etc.
    channel_config TEXT NOT NULL, -- JSON channel configuration (user_key, webhook_url, etc.)
    status TEXT NOT NULL DEFAULT 'pending', -- pending, sent, delivered, failed, retry
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 3,
    scheduled_at TEXT NOT NULL, -- when to send (for retries)
    sent_at TEXT,
    delivered_at TEXT,
    failed_at TEXT,
    error_message TEXT,
    external_id TEXT, -- external service message/delivery ID
    metadata TEXT, -- JSON delivery metadata
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (analysis_event_id) REFERENCES analysis_events(id) ON DELETE CASCADE
);

-- Indexes for notification delivery tracking
CREATE INDEX idx_notification_deliveries_event_id ON notification_deliveries(analysis_event_id);
CREATE INDEX idx_notification_deliveries_status ON notification_deliveries(status);
CREATE INDEX idx_notification_deliveries_channel ON notification_deliveries(channel_type);
CREATE INDEX idx_notification_deliveries_scheduled ON notification_deliveries(scheduled_at) WHERE status IN ('pending', 'retry');

-- Composite indexes for retry and failure tracking
CREATE INDEX idx_notification_deliveries_retry ON notification_deliveries(status, scheduled_at, attempt_count);
CREATE INDEX idx_notification_deliveries_failed ON notification_deliveries(status, failed_at) WHERE status = 'failed';
