-- Durable balance of calendar-window quotas. Short quotas and rate limits
-- stay in the state store; a row here survives a Redis flush or an edge
-- restart. `(bucket, window_start)` is the unit of accounting: a reservation
-- replaces the balance when the window changes, and the conditional upsert in
-- `QuotaRepository::reserve` keeps `used` at or below the limit across
-- replicas. The owner is the subject (a user or an API key) or organization
-- the bucket belongs to; balances of deleted owners are pruned in batches.
CREATE TABLE "quota_usage" (
    "bucket" TEXT PRIMARY KEY,
    "owner_kind" TEXT NOT NULL CHECK ("owner_kind" IN ('subject', 'organization')),
    "owner_id" TEXT NOT NULL,
    "definition" TEXT NOT NULL,
    "window_start" INTEGER NOT NULL,
    "window_end" INTEGER NOT NULL,
    "used" INTEGER NOT NULL DEFAULT 0 CHECK ("used" >= 0)
);
