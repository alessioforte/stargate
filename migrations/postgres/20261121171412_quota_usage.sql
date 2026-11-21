-- Durable balance of calendar-window quotas. Short quotas and rate limits
-- stay in the state store; a row here survives a Redis flush or an edge
-- restart. `(bucket, window_start)` is the unit of accounting: a reservation
-- replaces the balance when the window changes, and the conditional update in
-- `QuotaRepository::reserve` keeps `used` at or below the limit across
-- replicas.
CREATE TABLE "quota_usage" (
    "bucket" TEXT PRIMARY KEY,
    "definition" TEXT NOT NULL,
    "window_start" BIGINT NOT NULL,
    "window_end" BIGINT NOT NULL,
    "used" BIGINT NOT NULL DEFAULT 0 CHECK ("used" >= 0)
);
