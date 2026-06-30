PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS "organizations" (
    "id" TEXT PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
    "attrs" JSON NOT NULL DEFAULT '{}',
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS "service_accounts" (
    "id" TEXT PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
    "org_id" TEXT REFERENCES "organizations" ("id") ON DELETE CASCADE,
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
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
    "grant_types" JSON NOT NULL DEFAULT '[]',
    "response_types" JSON NOT NULL DEFAULT '[]',
    "redirect_uris" JSON NOT NULL DEFAULT '[]',
    "scopes" JSON NOT NULL DEFAULT '[]',
    "audiences" JSON NOT NULL DEFAULT '[]',
    "attrs" JSON NOT NULL DEFAULT '{}',
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS "users" (
    "id" TEXT PRIMARY KEY,
    "email" VARCHAR(100) NOT NULL UNIQUE,
    "given_name" VARCHAR(50),
    "family_name" VARCHAR(50),
    "nickname" VARCHAR(50) NOT NULL UNIQUE,
    "picture" TEXT,
    "phone_number" VARCHAR(20) UNIQUE,
    "attrs" JSON NOT NULL DEFAULT '{}',
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS "oauth_consents" (
    "id" TEXT PRIMARY KEY,
    "client_id" TEXT NOT NULL REFERENCES "oauth_clients" ("client_id") ON DELETE CASCADE,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "scopes" JSON NOT NULL DEFAULT '[]',
    "audiences" JSON NOT NULL DEFAULT '[]',
    "granted_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "expires_at" TEXT,
    "revoked_at" TEXT,
    "attrs" JSON NOT NULL DEFAULT '{}',
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE ("client_id", "user_id")
);

CREATE TABLE IF NOT EXISTS "super_admins" (
    "user_id" TEXT PRIMARY KEY REFERENCES "users" ("id") ON DELETE CASCADE,
    "active" BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS "credentials" (
    "id" TEXT PRIMARY KEY,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "type" VARCHAR(50) NOT NULL,
    "value" TEXT NOT NULL,
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS "api_keys" (
    "id" TEXT PRIMARY KEY,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100) NOT NULL,
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE,
    "attrs" JSON NOT NULL DEFAULT '{}',
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS "admin_keys" (
    "id" TEXT PRIMARY KEY,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100),
    "permissions" JSON NOT NULL DEFAULT '[]',
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE,
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS "audits" (
    "id" TEXT PRIMARY KEY,
    "timestamp" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "actor_type" VARCHAR(50) NOT NULL,
    "actor_id" TEXT,
    "action" VARCHAR(100) NOT NULL,
    "resource" VARCHAR(100) NOT NULL,
    "resource_id" TEXT,
    "request_id" TEXT,
    "metadata" JSON NOT NULL DEFAULT '{}'
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
