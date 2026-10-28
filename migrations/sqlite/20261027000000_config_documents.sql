CREATE TABLE IF NOT EXISTS "config_documents" (
    "id" TEXT PRIMARY KEY,
    "kind" TEXT NOT NULL CHECK ("kind" IN ('gateway', 'policies')),
    "version" INTEGER NOT NULL CHECK ("version" > 0),
    "revision" TEXT NOT NULL,
    "content" TEXT NOT NULL,
    "source" TEXT NOT NULL CHECK ("source" IN ('seed', 'admin_api', 'cli_import')),
    "created_by" TEXT,
    "created_at" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE ("kind", "version")
);
