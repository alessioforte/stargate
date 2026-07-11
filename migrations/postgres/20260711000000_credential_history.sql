-- Password history for the `not recently used` policy: previous password
-- hashes per user, appended on password change and pruned to a fixed cap.
CREATE TABLE IF NOT EXISTS "credential_history" (
    "id" TEXT PRIMARY KEY,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "value" TEXT NOT NULL,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS "idx_credential_history_user_created" ON "credential_history" ("user_id", "created_at" DESC);
