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
    "window_start" INTEGER NOT NULL,
    "window_end" INTEGER NOT NULL,
    "used" INTEGER NOT NULL DEFAULT 0 CHECK ("used" >= 0)
);

-- Only activation advances this version, so older activations cannot
-- restore retired definitions. SQLite serializes these writes.
CREATE TABLE "quota_config" (
    "id" INTEGER PRIMARY KEY CHECK ("id" = 1),
    "version" INTEGER NOT NULL CHECK ("version" >= 0)
);

INSERT INTO "quota_config" ("id", "version")
VALUES (1, 0);

CREATE TABLE "quota_definitions" (
    "name" TEXT PRIMARY KEY,
    "fingerprint" TEXT NOT NULL,
    "version" INTEGER NOT NULL CHECK ("version" > 0)
);
