-- Create templates table for capture configuration templates
CREATE TABLE templates (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    config TEXT NOT NULL, -- JSON configuration
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Indexes for template lookups
CREATE INDEX idx_templates_user_id ON templates(user_id);
CREATE INDEX idx_templates_name ON templates(name);
CREATE INDEX idx_templates_default ON templates(is_default);