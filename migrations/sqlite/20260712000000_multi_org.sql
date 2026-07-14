-- Multi-org (phase 1): membership roles + org-bound user API keys.
-- Roles are free-form strings with the convention 'owner' | 'admin' | 'member'.
ALTER TABLE "user_organizations" ADD COLUMN "role" TEXT NOT NULL DEFAULT 'member';

-- sqlite cannot ADD COLUMN with a non-constant default: add it nullable and
-- backfill. The repo layer always binds created_at explicitly on insert, so
-- rows never rely on a database default.
ALTER TABLE "user_organizations" ADD COLUMN "created_at" TEXT;
UPDATE "user_organizations" SET "created_at" = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE "created_at" IS NULL;

-- Optional org binding for user API keys: a bound key always acts in that
-- org. If the org is deleted the key degrades to an org-less key.
ALTER TABLE "user_api_keys" ADD COLUMN "org_id" TEXT REFERENCES "organizations" ("id") ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS "idx_user_api_keys_org_id" ON "user_api_keys" ("org_id");
