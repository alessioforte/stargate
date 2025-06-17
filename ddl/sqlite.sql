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

CREATE TABLE IF NOT EXISTS "actions" (
    `id` TEXT,
    `type` VARCHAR(50) NOT NULL,
    `sub` VARCHAR(100) NOT NULL,
    `value` TEXT NOT NULL,
    `iat` INT NOT NULL,
    `exp` INT NOT NULL,
    PRIMARY KEY (`id`)
);
