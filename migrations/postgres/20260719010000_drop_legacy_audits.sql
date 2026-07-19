-- The immutable `outbox_events` model and relay are fully cut over. Drop the
-- flattened audit model in a separate forward migration so previously applied
-- migration checksums remain valid.
DROP INDEX IF EXISTS "idx_audits_unpublished";
DROP INDEX IF EXISTS "idx_audits_resource";
DROP INDEX IF EXISTS "idx_audits_actor";
DROP INDEX IF EXISTS "idx_audits_timestamp";
DROP TABLE IF EXISTS "audits";

DROP TYPE IF EXISTS "action_type";
DROP TYPE IF EXISTS "actor_type";
