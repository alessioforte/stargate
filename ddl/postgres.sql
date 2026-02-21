CREATE TYPE credential_type AS ENUM ('password', 'oauth');
CREATE TYPE action_type AS ENUM ('create', 'update', 'delete', 'read', 'login', 'logout');
CREATE TYPE actor_type AS ENUM ('admin', 'user', 'api_key', 'system', 'anonymous');

CREATE TABLE IF NOT EXISTS "organizations" (
    "id" TEXT PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS "service_accounts" (
    "id" TEXT PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
    "org_id" TEXT REFERENCES "organizations" ("id") ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "users" (
    "id" TEXT PRIMARY KEY,
    "email" VARCHAR(100) NOT NULL UNIQUE,
    "given_name" VARCHAR(50),
    "family_name" VARCHAR(50),
    "nickname" VARCHAR(50) NOT NULL UNIQUE,
    "picture" TEXT,
    "phone_number" VARCHAR(20) UNIQUE,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS "credentials" (
    "id" TEXT PRIMARY KEY,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "type" credential_type NOT NULL,
    "value" TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "api_keys" (
    "id" TEXT PRIMARY KEY,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100) NOT NULL,
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS "admin_keys" (
    "id" TEXT PRIMARY KEY,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100),
    "permissions" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS "audits" (
    "id" TEXT PRIMARY KEY,
    "timestamp" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "actor_type" actor_type NOT NULL,
    "actor_id" TEXT,
    "action" action_type NOT NULL,
    "resource" VARCHAR(100) NOT NULL,
    "resource_id" TEXT,
    "request_id" TEXT,
    "metadata" JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS "user_organizations" (
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "org_id" TEXT NOT NULL REFERENCES "organizations" ("id") ON DELETE CASCADE,
    PRIMARY KEY ("user_id", "org_id")
);

CREATE TABLE IF NOT EXISTS "user_api_keys" (
    "api_key_id" TEXT PRIMARY KEY REFERENCES "api_keys" ("id") ON DELETE CASCADE,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "service_account_api_keys" (
    "api_key_id" TEXT PRIMARY KEY REFERENCES "api_keys" ("id") ON DELETE CASCADE,
    "service_account_id" TEXT NOT NULL REFERENCES "service_accounts" ("id") ON DELETE CASCADE
);

-- Indexes on foreign keys
CREATE INDEX IF NOT EXISTS "idx_service_accounts_org_id" ON "service_accounts" ("org_id");
CREATE INDEX IF NOT EXISTS "idx_credentials_user_id" ON "credentials" ("user_id");
CREATE INDEX IF NOT EXISTS "idx_user_api_keys_user_id" ON "user_api_keys" ("user_id");
CREATE INDEX IF NOT EXISTS "idx_sa_api_keys_service_account_id" ON "service_account_api_keys" ("service_account_id");
CREATE INDEX IF NOT EXISTS "idx_user_organizations_org_id" ON "user_organizations" ("org_id");

-- Indexes on audit query patterns
CREATE INDEX IF NOT EXISTS "idx_audits_timestamp" ON "audits" ("timestamp");
CREATE INDEX IF NOT EXISTS "idx_audits_actor" ON "audits" ("actor_type", "actor_id");
CREATE INDEX IF NOT EXISTS "idx_audits_resource" ON "audits" ("resource", "resource_id");
