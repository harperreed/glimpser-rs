-- Create a writable view `streams` backed by `templates`
-- This allows the codebase to transition to "streams" without a wide DB rename.

-- Drop existing view/triggers if re-running
DROP VIEW IF EXISTS streams;
DROP TRIGGER IF EXISTS streams_insert;
DROP TRIGGER IF EXISTS streams_update;
DROP TRIGGER IF EXISTS streams_delete;

-- Create view exposing templates as streams
CREATE VIEW streams AS
SELECT
    id,
    user_id,
    name,
    description,
    config,
    is_default,
    created_at,
    updated_at,
    execution_status,
    last_executed_at,
    last_error_message
FROM templates;

-- Writable triggers mapping streams writes to templates
CREATE TRIGGER streams_insert
INSTEAD OF INSERT ON streams
BEGIN
    INSERT INTO templates (
        id, user_id, name, description, config, is_default, created_at, updated_at,
        execution_status, last_executed_at, last_error_message
    ) VALUES (
        NEW.id, NEW.user_id, NEW.name, NEW.description, NEW.config, NEW.is_default, NEW.created_at, NEW.updated_at,
        COALESCE(NEW.execution_status, 'inactive'), NEW.last_executed_at, NEW.last_error_message
    );
END;

CREATE TRIGGER streams_update
INSTEAD OF UPDATE ON streams
BEGIN
    UPDATE templates SET
        user_id = NEW.user_id,
        name = NEW.name,
        description = NEW.description,
        config = NEW.config,
        is_default = NEW.is_default,
        created_at = NEW.created_at,
        updated_at = NEW.updated_at,
        execution_status = NEW.execution_status,
        last_executed_at = NEW.last_executed_at,
        last_error_message = NEW.last_error_message
    WHERE id = OLD.id;
END;

CREATE TRIGGER streams_delete
INSTEAD OF DELETE ON streams
BEGIN
    DELETE FROM templates WHERE id = OLD.id;
END;
