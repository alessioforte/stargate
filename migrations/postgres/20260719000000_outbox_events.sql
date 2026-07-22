-- Immutable raw audit events plus local relay state. The table is additive in
-- A2: legacy producers and the active relay continue to use `audits` until
-- their separately reviewed cutover phases.
CREATE TABLE "outbox_events" (
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
    "pair_role" VARCHAR(20),
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
