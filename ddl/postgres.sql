-- Enum types (idempotent: only created if they don't already exist)
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'credential_type') THEN
        CREATE TYPE credential_type AS ENUM ('password', 'oauth');
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'action_type') THEN
        CREATE TYPE action_type AS ENUM ('create', 'update', 'delete', 'read', 'login', 'logout');
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'actor_type') THEN
        CREATE TYPE actor_type AS ENUM ('admin', 'admin_key', 'user', 'api_key', 'system', 'anonymous');
    END IF;
END $$;

-- Add missing enum values to existing types (safe on repeated runs)
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_enum WHERE enumlabel = 'admin_key' AND enumtypid = 'actor_type'::regtype) THEN
        ALTER TYPE actor_type ADD VALUE IF NOT EXISTS 'admin_key';
    END IF;
END $$;

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

CREATE TABLE IF NOT EXISTS "oauth_clients" (
    "client_id" TEXT PRIMARY KEY,
    "client_secret_hash" TEXT UNIQUE,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
    "org_id" TEXT REFERENCES "organizations" ("id") ON DELETE SET NULL,
    "service_account_id" TEXT REFERENCES "service_accounts" ("id") ON DELETE SET NULL,
    "enabled" BOOLEAN NOT NULL DEFAULT TRUE,
    "token_endpoint_auth_method" TEXT NOT NULL,
    "grant_types" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "response_types" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "redirect_uris" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "scopes" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "audiences" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
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

CREATE TABLE IF NOT EXISTS "oauth_consents" (
    "id" TEXT PRIMARY KEY,
    "client_id" TEXT NOT NULL REFERENCES "oauth_clients" ("client_id") ON DELETE CASCADE,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "scopes" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "audiences" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "granted_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "expires_at" TIMESTAMPTZ,
    "revoked_at" TIMESTAMPTZ,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE ("client_id", "user_id")
);

CREATE TABLE IF NOT EXISTS "super_admins" (
    "user_id" TEXT PRIMARY KEY REFERENCES "users" ("id") ON DELETE CASCADE,
    "active" BOOLEAN NOT NULL DEFAULT TRUE
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
CREATE INDEX IF NOT EXISTS "idx_oauth_clients_org_id" ON "oauth_clients" ("org_id");
CREATE INDEX IF NOT EXISTS "idx_oauth_clients_service_account_id" ON "oauth_clients" ("service_account_id");
CREATE INDEX IF NOT EXISTS "idx_oauth_clients_enabled" ON "oauth_clients" ("enabled");
CREATE INDEX IF NOT EXISTS "idx_oauth_consents_user_client" ON "oauth_consents" ("user_id", "client_id");
CREATE INDEX IF NOT EXISTS "idx_oauth_consents_client_id" ON "oauth_consents" ("client_id");
CREATE INDEX IF NOT EXISTS "idx_oauth_consents_revoked_at" ON "oauth_consents" ("revoked_at");
CREATE INDEX IF NOT EXISTS "idx_credentials_user_id" ON "credentials" ("user_id");
CREATE INDEX IF NOT EXISTS "idx_super_admins_active" ON "super_admins" ("active");
CREATE INDEX IF NOT EXISTS "idx_user_api_keys_user_id" ON "user_api_keys" ("user_id");
CREATE INDEX IF NOT EXISTS "idx_sa_api_keys_service_account_id" ON "service_account_api_keys" ("service_account_id");
CREATE INDEX IF NOT EXISTS "idx_user_organizations_org_id" ON "user_organizations" ("org_id");

-- Indexes on audit query patterns
CREATE INDEX IF NOT EXISTS "idx_audits_timestamp" ON "audits" ("timestamp");
CREATE INDEX IF NOT EXISTS "idx_audits_actor" ON "audits" ("actor_type", "actor_id");
CREATE INDEX IF NOT EXISTS "idx_audits_resource" ON "audits" ("resource", "resource_id");
