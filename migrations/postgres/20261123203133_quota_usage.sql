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

-- Only activation advances this version. It serializes definition changes
-- and prevents a late activation from restoring retired definitions.
CREATE TABLE "quota_config" (
    "id" INTEGER PRIMARY KEY CHECK ("id" = 1),
    "version" BIGINT NOT NULL CHECK ("version" >= 0)
);

INSERT INTO "quota_config" ("id", "version")
VALUES (1, 0);

-- Reservations take a shared lock on just the definition they use. The
-- version is the first activated document in its current run.
CREATE TABLE "quota_definitions" (
    "name" TEXT PRIMARY KEY,
    "fingerprint" TEXT NOT NULL,
    "version" BIGINT NOT NULL CHECK ("version" > 0)
);
