-- Member pages filter by organization and walk user ids in descending order:
-- one index serves both, and covers every lookup by organization alone.
CREATE INDEX "idx_user_organizations_org_user" ON "user_organizations" ("org_id", "user_id");
DROP INDEX "idx_user_organizations_org_id";
