CREATE TABLE IF NOT EXISTS "accounts" (
    `id` TEXT,
    `type` VARCHAR(50) NOT NULL, -- e.g., 'user', 'service_account  etc.'
    `name` VARCHAR(100) NOT NULL,
    `description` TEXT,
    PRIMARY KEY (`id`)
);

CREATE TABLE IF NOT EXISTS "users" (
    `id` TEXT,
    `account_id` TEXT NOT NULL,
    `email` VARCHAR(100) NOT NULL UNIQUE,
    `given_name` VARCHAR(50),
    `family_name` VARCHAR(50),
    `nickname` VARCHAR(50) NOT NULL UNIQUE,
    `picture` TEXT,
    `phone_number` VARCHAR(20) UNIQUE,
    `attrs` JSON NOT NULL DEFAULT '{}',
    PRIMARY KEY (`id`) FOREIGN KEY (`account_id`) REFERENCES "accounts" (`id`) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "credentials" (
    `id` TEXT,
    `user_id` TEXT NOT NULL,
    `timestamp` INT NOT NULL,
    `type` VARCHAR(50) NOT NULL,
    `value` TEXT NOT NULL,
    PRIMARY KEY (`id`) FOREIGN KEY (`user_id`) REFERENCES "users" (`id`) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "api_keys" (
    `id` TEXT,
    `account_id` TEXT NOT NULL,
    `key_hash` TEXT NOT NULL UNIQUE,
    `label` VARCHAR(100) NOT NULL,
    `revoked` BOOLEAN NOT NULL DEFAULT FALSE,
    `attrs` JSON NOT NULL DEFAULT '{}',
    PRIMARY KEY (`id`) FOREIGN KEY (`account_id`) REFERENCES "accounts" (`id`) ON DELETE CASCADE
);

-- CREATE TABLE IF NOT EXISTS "audits" (
--     `id` TEXT,
--     `timestamp` INT NOT NULL,
--     `entity_type` VARCHAR(50) NOT NULL,
--     `entity_id` TEXT NOT NULL,
--     `action` VARCHAR(50) NOT NULL,
--     `performed_by` TEXT,
--     `details` JSON NOT NULL DEFAULT '{}',
--     PRIMARY KEY (`id`)
-- );
