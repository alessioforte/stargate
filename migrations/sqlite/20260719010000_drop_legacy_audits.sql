-- The immutable `outbox_events` model is fully cut over. SQLite drops table
-- indexes automatically, but name them explicitly to document the cleanup and
-- make the migration safe against partially initialized development schemas.
DROP INDEX IF EXISTS "idx_audits_resource";
DROP INDEX IF EXISTS "idx_audits_actor";
DROP INDEX IF EXISTS "idx_audits_timestamp";
DROP TABLE IF EXISTS "audits";
