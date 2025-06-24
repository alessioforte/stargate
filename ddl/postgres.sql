CREATE TABLE IF NOT EXISTS "users" (
    "id" TEXT PRIMARY KEY,
    "email" VARCHAR(100) NOT NULL UNIQUE,
    "first_name" VARCHAR(50),
    "last_name" VARCHAR(50),
    "nickname" VARCHAR(50) NOT NULL UNIQUE,
    "picture" TEXT,
    "phone_number" VARCHAR(20) UNIQUE
);

CREATE TABLE IF NOT EXISTS "credentials" (
    "id" TEXT PRIMARY KEY,
    "user_id" TEXT NOT NULL REFERENCES "users" ("id") ON DELETE CASCADE,
    "timestamp" bigint NOT NULL,
    "type" VARCHAR(50) NOT NULL,
    "value" TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "actions" (
    "id" TEXT PRIMARY KEY,
    "type" VARCHAR(50) NOT NULL,
    "sub" VARCHAR(100) NOT NULL,
    "value" TEXT NOT NULL,
    "iat" bigint NOT NULL,
    "exp" bigint NOT NULL
);
