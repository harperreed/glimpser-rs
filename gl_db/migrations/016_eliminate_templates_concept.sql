-- Eliminate the templates concept entirely - only streams exist
-- This migration renames the templates table to streams and removes the compatibility view

-- Step 1: Drop the compatibility view and its triggers
DROP TRIGGER IF EXISTS streams_delete;
DROP TRIGGER IF EXISTS streams_update;
DROP TRIGGER IF EXISTS streams_insert;
DROP VIEW IF EXISTS streams;

-- Step 2: Rename templates table to streams
ALTER TABLE templates RENAME TO streams;

-- Step 3: Drop old indexes
DROP INDEX IF EXISTS idx_templates_user_id;
DROP INDEX IF EXISTS idx_templates_name;
DROP INDEX IF EXISTS idx_templates_default;
DROP INDEX IF EXISTS idx_templates_execution_status;

-- Step 4: Create new indexes with proper naming
CREATE INDEX IF NOT EXISTS idx_streams_user_id ON streams(user_id);
CREATE INDEX IF NOT EXISTS idx_streams_name ON streams(name);
CREATE INDEX IF NOT EXISTS idx_streams_default ON streams(is_default);
CREATE INDEX IF NOT EXISTS idx_streams_execution_status ON streams(execution_status);

-- Note: No data migration needed - just a rename!
