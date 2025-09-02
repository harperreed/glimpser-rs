-- Add execution state tracking fields to templates table
ALTER TABLE templates ADD COLUMN execution_status VARCHAR(20) DEFAULT 'inactive';
ALTER TABLE templates ADD COLUMN last_executed_at TEXT;
ALTER TABLE templates ADD COLUMN last_error_message TEXT;

-- Create index for execution status queries
CREATE INDEX idx_templates_execution_status ON templates(execution_status);
