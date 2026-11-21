-- no-transaction
-- RFC 5321 §4.5.3.1.3: a forward-path is at most 256 octets including the
-- angle brackets, so an address is at most 254. The default nickname is the
-- email, so it needs the same room.
--
-- SQLite does not enforce VARCHAR lengths, so the columns are rebuilt to keep
-- the schema identical to PostgreSQL. Foreign keys are disabled outside the
-- transaction (the documented procedure), so DROP TABLE does not cascade into
-- the child tables; the explicit transaction makes the rebuild atomic.
PRAGMA foreign_keys = OFF;

BEGIN;

CREATE TABLE "users_new" (
    "id" TEXT PRIMARY KEY,
    "email" VARCHAR(254) NOT NULL UNIQUE,
    "given_name" VARCHAR(50),
    "family_name" VARCHAR(50),
    "nickname" VARCHAR(254) NOT NULL UNIQUE,
    "picture" TEXT,
    "phone_number" VARCHAR(20) UNIQUE,
    "attrs" JSON NOT NULL DEFAULT '{}',
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updated_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO "users_new" (
    "id",
    "email",
    "given_name",
    "family_name",
    "nickname",
    "picture",
    "phone_number",
    "attrs",
    "created_at",
    "updated_at"
)
SELECT
    "id",
    "email",
    "given_name",
    "family_name",
    "nickname",
    "picture",
    "phone_number",
    "attrs",
    "created_at",
    "updated_at"
FROM "users";

DROP TABLE "users";
ALTER TABLE "users_new" RENAME TO "users";

COMMIT;

PRAGMA foreign_keys = ON;
