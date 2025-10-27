
-- CREATE TYPE account_type AS ENUM ('user', 'service');
-- CREATE TYPE credential_type AS ENUM ('password', 'oauth');

CREATE TABLE IF NOT EXISTS "accounts" (
    "id" TEXT PRIMARY KEY,
    "type" account_type NOT NULL,
    -- "type" VARCHAR(50) NOT NULL,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT
);

CREATE TABLE IF NOT EXISTS "users" (
    "id" TEXT PRIMARY KEY,
    "account_id" TEXT NOT NULL REFERENCES "accounts" ("id") ON DELETE CASCADE,
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
    "timestamp" bigint NOT NULL,
    -- "type" VARCHAR(50) NOT NULL,
    "type" credential_type NOT NULL,
    "value" TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "api_keys" (
    "id" TEXT PRIMARY KEY,
    "account_id" TEXT NOT NULL REFERENCES "accounts" ("id") ON DELETE CASCADE,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100) NOT NULL,
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb
);

-- CREATE TABLE IF NOT EXISTS "audits" (
--     "id" TEXT PRIMARY KEY,
--     "timestamp" bigint NOT NULL,
--     "entity_type" VARCHAR(50) NOT NULL,
--     "entity_id" TEXT NOT NULL,
--     "action" VARCHAR(50) NOT NULL,
--     "performed_by" TEXT,
--     "details" JSONB NOT NULL DEFAULT '{}'::jsonb
-- );

-- Create indexes for performance optimization
-- CREATE INDEX IF NOT EXISTS "idx_users_email" ON "users" ("email");
-- CREATE INDEX IF NOT EXISTS "idx_users_nickname" ON "users" ("nickname");
-- CREATE INDEX IF NOT EXISTS "idx_credentials_user_id" ON "credentials" ("user_id");
-- CREATE INDEX IF NOT EXISTS "idx_actions_sub" ON "actions" ("sub");
-- CREATE INDEX IF NOT EXISTS "idx_api_keys_user_id" ON "api_keys" ("user_id");
-- CREATE INDEX IF NOT EXISTS "idx_api_keys_key_hash" ON "api_keys" ("key_hash");
