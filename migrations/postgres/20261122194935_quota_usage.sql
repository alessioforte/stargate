-- Durable balance of calendar-window quotas. Short quotas and rate limits
-- stay in the state store; a row here survives a Redis flush or an edge
-- restart. Each row keeps the active bounds until the window ends, even
-- when the configured calendar changes. The conditional upsert enforces
-- the limit across replicas. Balances of deleted subjects (users or API keys)
-- and organizations are pruned in batches.
CREATE TABLE "quota_usage" (
    "bucket" TEXT PRIMARY KEY,
    "owner_kind" TEXT NOT NULL CHECK ("owner_kind" IN ('subject', 'organization')),
    "owner_id" TEXT NOT NULL,
    "window_start" BIGINT NOT NULL,
    "window_end" BIGINT NOT NULL,
    "used" BIGINT NOT NULL DEFAULT 0 CHECK ("used" >= 0)
);

-- A reservation takes a shared lock on this row. A gateway configuration
-- save updates it in the same transaction as the new document version.
CREATE TABLE "quota_config" (
    "id" INTEGER PRIMARY KEY CHECK ("id" = 1),
    "version" BIGINT NOT NULL CHECK ("version" >= 0)
);

INSERT INTO "quota_config" ("id", "version")
SELECT 1, COALESCE(MAX("version"), 0) FROM "config_documents" WHERE "kind" = 'gateway';

CREATE FUNCTION advance_quota_config_version() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE quota_config SET version = NEW.version WHERE id = 1 AND version < NEW.version;
    RETURN NEW;
END;
$$;

CREATE TRIGGER advance_quota_config_version
AFTER INSERT ON config_documents FOR EACH ROW WHEN (NEW.kind = 'gateway')
EXECUTE FUNCTION advance_quota_config_version();
