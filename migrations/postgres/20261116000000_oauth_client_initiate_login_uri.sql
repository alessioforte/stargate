-- OpenID Connect Core 1.0 §4 (Initiating Login from a Third Party): the
-- endpoint an app registers to start a login, and where Stargate sends a user
-- once an invitation, a registration, or a password reset that began in that
-- app completes.
ALTER TABLE "oauth_clients" ADD COLUMN "initiate_login_uri" TEXT;
