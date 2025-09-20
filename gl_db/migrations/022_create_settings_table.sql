-- Create settings table for configurable system parameters
-- Each setting has a key, value, category, and metadata

CREATE TABLE IF NOT EXISTS settings (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'system',
    description TEXT,
    data_type TEXT NOT NULL DEFAULT 'string',
    min_value REAL,
    max_value REAL,
    default_value TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Index for fast lookups by key
CREATE INDEX idx_settings_key ON settings(key);
CREATE INDEX idx_settings_category ON settings(category);

-- Insert default settings
INSERT INTO settings (id, key, value, category, description, data_type, min_value, max_value, default_value, created_at, updated_at) VALUES
    ('01JSETTINGS_PHASH_THRESHOLD', 'phash_similarity_threshold', '0.85', 'image_processing', 'Perceptual hash similarity threshold (0.0-1.0). Lower = more sensitive to changes', 'float', 0.0, 1.0, '0.85', datetime('now'), datetime('now')),
    ('01JSETTINGS_SNAPSHOT_RETENTION', 'snapshot_retention_count', '100', 'storage', 'Number of snapshots to keep per stream (0 = unlimited)', 'integer', 0.0, 10000.0, '100', datetime('now'), datetime('now')),
    ('01JSETTINGS_HASH_ALGORITHM', 'phash_algorithm', 'gradient', 'image_processing', 'Perceptual hash algorithm to use', 'enum:gradient,mean,block,median', NULL, NULL, 'gradient', datetime('now'), datetime('now')),
    ('01JSETTINGS_HASH_SIZE', 'phash_hash_size', '8', 'image_processing', 'Hash size (NxN) - higher = more detailed but slower', 'integer', 4.0, 32.0, '8', datetime('now'), datetime('now')),
    ('01JSETTINGS_AUTO_CLEANUP', 'auto_cleanup_enabled', 'true', 'storage', 'Automatically cleanup old snapshots based on retention count', 'boolean', NULL, NULL, 'true', datetime('now'), datetime('now')),
    ('01JSETTINGS_DEFAULT_INTERVAL', 'default_capture_interval', '30', 'capture', 'Default snapshot interval in seconds for new streams', 'integer', 1.0, 86400.0, '30', datetime('now'), datetime('now'));
