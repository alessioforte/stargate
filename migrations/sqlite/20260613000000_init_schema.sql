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
    "enabled" BOOLEAN NOT NULL DEFAULT TRUE,
    "token_endpoint_auth_method" TEXT NOT NULL,
    -- Public keys of private_key_jwt clients (OIDC Core 9, RFC 7523): an
    -- inline JWK Set or a URL that serves one.
    "jwks" JSON,
    "jwks_uri" TEXT,
    "grant_types" JSON NOT NULL DEFAULT '[]',
    "response_types" JSON NOT NULL DEFAULT '[]',
    "redirect_uris" JSON NOT NULL DEFAULT '[]',
    -- RP-Initiated Logout 1.0 and Back-Channel Logout 1.0 metadata.
    "post_logout_redirect_uris" JSON NOT NULL DEFAULT '[]',
    "backchannel_logout_uri" TEXT,
    "backchannel_logout_session_required" BOOLEAN NOT NULL DEFAULT FALSE,
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

-- Password history for the `not recently used` policy: previous password
-- hashes per user, appended on password change and pruned to a fixed cap.
CREATE TABLE IF NOT EXISTS "credential_history" (
    "id" TEXT PRIMARY KEY,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "value" TEXT NOT NULL,
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Memberships carry a role, free-form with the convention
-- 'owner' | 'admin' | 'member'.
CREATE TABLE IF NOT EXISTS "user_organizations" (
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "org_id" TEXT NOT NULL REFERENCES "organizations" ("id") ON DELETE CASCADE,
    "role" TEXT NOT NULL DEFAULT 'member',
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
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

-- Immutable raw audit events plus local delivery metadata, the same logical
-- columns as PostgreSQL. Edge starts no relay, so published_at stays NULL.
CREATE TABLE IF NOT EXISTS "outbox_events" (
    "seq" INTEGER PRIMARY KEY AUTOINCREMENT,
    "event_id" TEXT NOT NULL UNIQUE
        CHECK (length("event_id") = 26)
        CHECK ("event_id" NOT GLOB '*[^0-9A-HJKMNP-TV-Z]*'),
    "payload" TEXT NOT NULL
        CHECK (length(CAST("payload" AS BLOB)) BETWEEN 2 AND 65536)
        CHECK (json_valid("payload") = 1)
        CHECK (json_type("payload") = 'object')
        CHECK (json_type("payload", '$.event_id') = 'text')
        CHECK (json_extract("payload", '$.event_id') = "event_id"),
    "operation_id" TEXT
        CHECK (
            "operation_id" IS NULL
            OR (
                length("operation_id") = 26
                AND "operation_id" NOT GLOB '*[^0-9A-HJKMNP-TV-Z]*'
            )
        ),
    "created_at" TEXT NOT NULL
        DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "published_at" TEXT,
    CHECK (
        json_type("payload", '$.operation_id') IS NOT NULL
        AND (
            (
                "operation_id" IS NULL
                AND json_type("payload", '$.operation_id') = 'null'
            )
            OR (
                "operation_id" IS NOT NULL
                AND json_type("payload", '$.operation_id') = 'text'
                AND json_extract("payload", '$.operation_id') = "operation_id"
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

-- Outbox delivery and correlation
CREATE INDEX IF NOT EXISTS "idx_outbox_events_unpublished"
ON "outbox_events" ("seq")
WHERE "published_at" IS NULL;
CREATE INDEX IF NOT EXISTS "idx_outbox_events_operation_id"
ON "outbox_events" ("operation_id")
WHERE "operation_id" IS NOT NULL;
