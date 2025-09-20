-- Add perceptual hash column to snapshots table for duplicate detection
-- This enables smart snapshot comparison to avoid storing unchanged frames

ALTER TABLE snapshots ADD COLUMN perceptual_hash TEXT;

-- Create index for efficient hash lookups during duplicate detection
CREATE INDEX idx_snapshots_perceptual_hash ON snapshots(perceptual_hash);

-- Create composite index for efficient stream-specific hash lookups
CREATE INDEX idx_snapshots_stream_hash ON snapshots(stream_id, perceptual_hash);
