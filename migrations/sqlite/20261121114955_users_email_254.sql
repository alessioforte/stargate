-- no-transaction
-- RFC 5321 §4.5.3.1.3: a forward-path is at most 256 octets including the
-- angle brackets, so an address is at most 254.
--
-- SQLite does not enforce VARCHAR lengths, so the column is rebuilt to keep
-- the schema identical to PostgreSQL. The rebuild follows the documented
-- procedure: foreign keys are disabled around it, so DROP TABLE does not
-- cascade into the child tables, and the old and new tables keep the same ids.
PRAGMA foreign_keys = OFF;

CREATE TABLE "users_new" (
    "id" TEXT PRIMARY KEY,
    "email" VARCHAR(254) NOT NULL UNIQUE,
    "given_name" VARCHAR(50),
    "family_name" VARCHAR(50),
    "nickname" VARCHAR(50) NOT NULL UNIQUE,
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

PRAGMA foreign_keys = ON;
