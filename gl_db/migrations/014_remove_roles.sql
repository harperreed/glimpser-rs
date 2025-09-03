-- Remove role and is_admin columns - no admin functionality needed
-- This completes the auth simplification by removing all role-related fields

-- SQLite doesn't support DROP COLUMN IF EXISTS, so we need to recreate the table
-- Create new users table without role/is_admin columns
CREATE TABLE users_new (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_active BOOLEAN DEFAULT true,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Copy data from old table (excluding role/is_admin columns)
INSERT INTO users_new (id, username, email, password_hash, is_active, created_at, updated_at)
SELECT id, username, email, password_hash, is_active, created_at, updated_at FROM users;

-- Drop old table and rename new table
DROP TABLE users;
ALTER TABLE users_new RENAME TO users;
