-- SQLite stores the same immutable raw event and local delivery metadata as
-- PostgreSQL. Edge starts no relay, so published_at remains NULL until a future
-- separately approved relay is implemented.
CREATE TABLE "outbox_events" (
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
    "pair_role" TEXT,
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
    ),
    CHECK (
        ("operation_id" IS NULL AND "pair_role" IS NULL)
        OR (
            "operation_id" IS NOT NULL
            AND "pair_role" IN ('target', 'control_plane')
        )
    )
);

CREATE INDEX "idx_outbox_events_unpublished"
ON "outbox_events" ("seq")
WHERE "published_at" IS NULL;

CREATE UNIQUE INDEX "idx_outbox_events_pair_role"
ON "outbox_events" ("operation_id", "pair_role")
WHERE "operation_id" IS NOT NULL;
