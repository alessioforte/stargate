CREATE TYPE credential_type AS ENUM ('password', 'oauth');

CREATE TABLE IF NOT EXISTS "organizations" (
    "id" TEXT PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS "service_accounts" (
    "id" TEXT PRIMARY KEY,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
    "org_id" TEXT REFERENCES "organizations" ("id") ON DELETE CASCADE,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS "oauth_clients" (
    "client_id" TEXT PRIMARY KEY,
    "client_secret_hash" TEXT UNIQUE,
    "name" VARCHAR(100) NOT NULL,
    "description" TEXT,
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
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
    "value" TEXT NOT NULL,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS "api_keys" (
    "id" TEXT PRIMARY KEY,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100) NOT NULL,
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE,
    "attrs" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS "admin_keys" (
    "id" TEXT PRIMARY KEY,
    "key_hash" TEXT NOT NULL UNIQUE,
    "label" VARCHAR(100),
    "permissions" JSONB NOT NULL DEFAULT '[]'::jsonb,
    "revoked" BOOLEAN NOT NULL DEFAULT FALSE,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Password history for the `not recently used` policy: previous password
-- hashes per user, appended on password change and pruned to a fixed cap.
CREATE TABLE IF NOT EXISTS "credential_history" (
    "id" TEXT PRIMARY KEY,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "value" TEXT NOT NULL,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Memberships carry a role, free-form with the convention
-- 'owner' | 'admin' | 'member'.
CREATE TABLE IF NOT EXISTS "user_organizations" (
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "org_id" TEXT NOT NULL REFERENCES "organizations" ("id") ON DELETE CASCADE,
    "role" TEXT NOT NULL DEFAULT 'member',
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY ("user_id", "org_id")
);

-- Optional org binding: a bound key always acts in that org. If the org is
-- deleted the key degrades to an org-less key.
CREATE TABLE IF NOT EXISTS "user_api_keys" (
    "api_key_id" TEXT PRIMARY KEY REFERENCES "api_keys" ("id") ON DELETE CASCADE,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "org_id" TEXT REFERENCES "organizations" ("id") ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS "service_account_api_keys" (
    "api_key_id" TEXT PRIMARY KEY REFERENCES "api_keys" ("id") ON DELETE CASCADE,
    "service_account_id" TEXT NOT NULL REFERENCES "service_accounts" ("id") ON DELETE CASCADE
);

-- Immutable raw audit events, inserted in the same transaction as the mutation
-- they record. `seq` is the relay's claim cursor; `published_at` stays NULL until
-- a relay node ships the row to Redis Streams (cluster deployments only).
CREATE TABLE IF NOT EXISTS "outbox_events" (
    "event_id" CHAR(26) PRIMARY KEY
        CHECK ("event_id" ~ '^[0-9A-HJKMNP-TV-Z]{26}$'),
    "payload" TEXT NOT NULL
        CHECK (octet_length("payload") BETWEEN 2 AND 65536)
        CHECK (jsonb_typeof("payload"::jsonb) = 'object')
        CHECK (
            "payload"::jsonb ? 'event_id'
            AND jsonb_typeof("payload"::jsonb -> 'event_id') = 'string'
            AND "payload"::jsonb ->> 'event_id' = btrim("event_id")
        ),
    "seq" BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    "operation_id" CHAR(26)
        CHECK (
            "operation_id" IS NULL
            OR "operation_id" ~ '^[0-9A-HJKMNP-TV-Z]{26}$'
        ),
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "published_at" TIMESTAMPTZ,
    CHECK (
        "payload"::jsonb ? 'operation_id'
        AND (
            (
                "operation_id" IS NULL
                AND jsonb_typeof("payload"::jsonb -> 'operation_id') = 'null'
            )
            OR (
                "operation_id" IS NOT NULL
                AND jsonb_typeof("payload"::jsonb -> 'operation_id') = 'string'
                AND "payload"::jsonb ->> 'operation_id' = btrim("operation_id")
            )
        )
    )
);

-- Indexes on foreign keys
CREATE INDEX IF NOT EXISTS "idx_service_accounts_org_id" ON "service_accounts" ("org_id");
CREATE INDEX IF NOT EXISTS "idx_oauth_clients_enabled" ON "oauth_clients" ("enabled");
CREATE INDEX IF NOT EXISTS "idx_oauth_consents_user_client" ON "oauth_consents" ("user_id", "client_id");
CREATE INDEX IF NOT EXISTS "idx_oauth_consents_client_id" ON "oauth_consents" ("client_id");
CREATE INDEX IF NOT EXISTS "idx_oauth_consents_revoked_at" ON "oauth_consents" ("revoked_at");
CREATE INDEX IF NOT EXISTS "idx_credentials_user_id" ON "credentials" ("user_id");
CREATE INDEX IF NOT EXISTS "idx_credential_history_user_created" ON "credential_history" ("user_id", "created_at" DESC);
CREATE INDEX IF NOT EXISTS "idx_super_admins_active" ON "super_admins" ("active");
CREATE INDEX IF NOT EXISTS "idx_user_api_keys_user_id" ON "user_api_keys" ("user_id");
CREATE INDEX IF NOT EXISTS "idx_user_api_keys_org_id" ON "user_api_keys" ("org_id");
CREATE INDEX IF NOT EXISTS "idx_sa_api_keys_service_account_id" ON "service_account_api_keys" ("service_account_id");
CREATE INDEX IF NOT EXISTS "idx_user_organizations_org_id" ON "user_organizations" ("org_id");

-- Outbox claim index: the relay's "next unpublished rows by seq" scan. Partial on
-- published_at IS NULL, so it only holds the unpublished backlog.
CREATE INDEX IF NOT EXISTS "idx_outbox_events_unpublished"
ON "outbox_events" ("seq")
WHERE "published_at" IS NULL;
CREATE INDEX IF NOT EXISTS "idx_outbox_events_operation_id"
ON "outbox_events" ("operation_id")
WHERE "operation_id" IS NOT NULL;
