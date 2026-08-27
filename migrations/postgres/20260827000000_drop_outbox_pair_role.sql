-- Correlated audit operations may contain any number of events. The nullable
-- operation_id remains the correlation key; scope and service identify each
-- event without a separate pair role.
ALTER TABLE "outbox_events"
DROP COLUMN "pair_role";

CREATE INDEX "idx_outbox_events_operation_id"
ON "outbox_events" ("operation_id")
WHERE "operation_id" IS NOT NULL;
