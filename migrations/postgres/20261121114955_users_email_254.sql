-- RFC 5321 §4.5.3.1.3: a forward-path is at most 256 octets including the
-- angle brackets, so an address is at most 254.
ALTER TABLE "users" ALTER COLUMN "email" TYPE VARCHAR(254);
