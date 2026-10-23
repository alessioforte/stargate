-- Public keys of clients that authenticate with private_key_jwt (OIDC Core 9,
-- RFC 7523): an inline JWK Set or a URL that serves one.
ALTER TABLE "oauth_clients" ADD COLUMN "jwks" JSONB;
ALTER TABLE "oauth_clients" ADD COLUMN "jwks_uri" TEXT;
