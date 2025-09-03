-- Simplify user roles from string enum to boolean is_admin
-- Convert existing roles: admin -> true, all others -> false

-- Add new is_admin column as boolean
ALTER TABLE users ADD COLUMN is_admin BOOLEAN DEFAULT false;

-- Convert existing role data
UPDATE users SET is_admin = true WHERE role = 'admin';
UPDATE users SET is_admin = false WHERE role != 'admin';

-- Since we don't need backward compatibility, we would recreate the table
-- to drop the role column, but for now this migration adds is_admin
