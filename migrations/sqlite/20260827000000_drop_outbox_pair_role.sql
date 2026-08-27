-- SQLite cannot drop pair_role while the original table-level CHECK refers to
-- it, so rebuild the table while preserving every event and sequence number.
CREATE TABLE "outbox_events_without_pair_role" (
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

INSERT INTO "outbox_events_without_pair_role" (
    "seq",
    "event_id",
    "payload",
    "operation_id",
    "created_at",
    "published_at"
)
SELECT
    "seq",
    "event_id",
    "payload",
    "operation_id",
    "created_at",
    "published_at"
FROM "outbox_events";

DROP TABLE "outbox_events";

ALTER TABLE "outbox_events_without_pair_role"
RENAME TO "outbox_events";

CREATE INDEX "idx_outbox_events_unpublished"
ON "outbox_events" ("seq")
WHERE "published_at" IS NULL;

CREATE INDEX "idx_outbox_events_operation_id"
ON "outbox_events" ("operation_id")
WHERE "operation_id" IS NOT NULL;
