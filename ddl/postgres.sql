CREATE TABLE IF NOT EXISTS "users" (
    "id" TEXT PRIMARY KEY,
    "email" VARCHAR(100) NOT NULL UNIQUE,
    "first_name" VARCHAR(50),
    "last_name" VARCHAR(50),
    "nickname" VARCHAR(50) NOT NULL UNIQUE,
    "picture" TEXT,
    "phone_number" VARCHAR(20) UNIQUE
);

CREATE TABLE IF NOT EXISTS "credentials" (
    "id" TEXT PRIMARY KEY,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "timestamp" bigint NOT NULL,
    "type" VARCHAR(50) NOT NULL,
    "value" TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "service_accounts" (
    "id" TEXT PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL UNIQUE,
    "description" TEXT,
);

CREATE TABLE IF NOT EXISTS "actions" (
    "id" TEXT PRIMARY KEY,
    "type" VARCHAR(50) NOT NULL,
    "sub" VARCHAR(100) NOT NULL,
    "value" TEXT NOT NULL,
    "iat" bigint NOT NULL,
    "exp" bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS "api_keys" (
    "id" TEXT PRIMARY KEY,
    "owner" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "owner_type" VARCHAR(50) NOT NULL,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100) NOT NULL,
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE,
    "exp" bigint
);

CREATE TABLE IF NOT EXISTS "subjects" (
    "id" TEXT PRIMARY KEY,
    "type" VARCHAR(50) NOT NULL,
    "sub_id" TEXT NOT NULL,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "limits" JSONB DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS "audits" (
    "id" TEXT PRIMARY KEY,
    "timestamp" bigint NOT NULL,
    "entity_type" VARCHAR(50) NOT NULL,
    "entity_id" TEXT NOT NULL,
    "action" VARCHAR(50) NOT NULL,
    "performed_by" TEXT,
    "details" JSONB NOT NULL DEFAULT '{}'::jsonb
);

-- Create indexes for performance optimization
-- CREATE INDEX IF NOT EXISTS "idx_users_email" ON "users" ("email");
-- CREATE INDEX IF NOT EXISTS "idx_users_nickname" ON "users" ("nickname");
-- CREATE INDEX IF NOT EXISTS "idx_credentials_user_id" ON "credentials" ("user_id");
-- CREATE INDEX IF NOT EXISTS "idx_actions_sub" ON "actions" ("sub");
-- CREATE INDEX IF NOT EXISTS "idx_api_keys_user_id" ON "api_keys" ("user_id");
-- CREATE INDEX IF NOT EXISTS "idx_api_keys_key_hash" ON "api_keys" ("key_hash");
