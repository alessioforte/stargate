-- Multi-org (phase 1): membership roles + org-bound user API keys.
-- Roles are free-form strings with the convention 'owner' | 'admin' | 'member'.
ALTER TABLE "user_organizations" ADD COLUMN IF NOT EXISTS "role" TEXT NOT NULL DEFAULT 'member';
ALTER TABLE "user_organizations" ADD COLUMN IF NOT EXISTS "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW();

-- Optional org binding for user API keys: a bound key always acts in that
-- org. If the org is deleted the key degrades to an org-less key.
ALTER TABLE "user_api_keys" ADD COLUMN IF NOT EXISTS "org_id" TEXT REFERENCES "organizations" ("id") ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS "idx_user_api_keys_org_id" ON "user_api_keys" ("org_id");
