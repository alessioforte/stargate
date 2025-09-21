CREATE TABLE IF NOT EXISTS "users" (
    `id` TEXT,
    `email` VARCHAR(100) NOT NULL UNIQUE,
    `first_name` VARCHAR(50),
    `last_name` VARCHAR(50),
    `nickname` VARCHAR(50) NOT NULL UNIQUE,
    `picture` TEXT,
    `phone_number` VARCHAR(20) UNIQUE,
    PRIMARY KEY (`id`)
);

CREATE TABLE IF NOT EXISTS "credentials" (
    `id` TEXT,
    `user_id` TEXT NOT NULL,
    `timestamp` INT NOT NULL,
    `type` VARCHAR(50) NOT NULL,
    `value` TEXT NOT NULL,
    PRIMARY KEY (`id`) FOREIGN KEY (`user_id`) REFERENCES "users" (`id`) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "service_accounts" (
    `id` TEXT,
    `name` VARCHAR(100) NOT NULL UNIQUE,
    `description` TEXT,
    PRIMARY KEY (`id`)
);

CREATE TABLE IF NOT EXISTS "actions" (
    `id` TEXT,
    `type` VARCHAR(50) NOT NULL,
    `sub` VARCHAR(100) NOT NULL,
    `value` TEXT NOT NULL,
    `iat` INT NOT NULL,
    `exp` INT NOT NULL,
    PRIMARY KEY (`id`)
);

CREATE TABLE IF NOT EXISTS "api_keys" (
    `id` TEXT,
    `owner` TEXT NOT NULL,
    `owner_type` VARCHAR(50) NOT NULL,
    `key_hash` TEXT NOT NULL UNIQUE,
    `label` VARCHAR(100) NOT NULL,
    `revoked` BOOLEAN NOT NULL DEFAULT FALSE,
    `exp` INT,
    PRIMARY KEY (`id`) FOREIGN KEY (`owner`) REFERENCES "users" (`id`) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "subjects" (
    `id` TEXT,
    `type` VARCHAR(50) NOT NULL,
    `sub_id` TEXT NOT NULL,
    `attrs` JSON NOT NULL DEFAULT '{}',
    PRIMARY KEY (`id`)
);

CREATE TABLE IF NOT EXISTS "audits" (
    `id` TEXT,
    `timestamp` INT NOT NULL,
    `entity_type` VARCHAR(50) NOT NULL,
    `entity_id` TEXT NOT NULL,
    `action` VARCHAR(50) NOT NULL,
    `performed_by` TEXT,
    `details` JSON NOT NULL DEFAULT '{}',
    PRIMARY KEY (`id`)
);
