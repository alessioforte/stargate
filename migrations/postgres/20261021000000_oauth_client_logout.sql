-- OpenID Connect RP-Initiated Logout 1.0 and Back-Channel Logout 1.0 client metadata.
ALTER TABLE "oauth_clients"
    ADD COLUMN "post_logout_redirect_uris" JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE "oauth_clients" ADD COLUMN "backchannel_logout_uri" TEXT;
ALTER TABLE "oauth_clients"
    ADD COLUMN "backchannel_logout_session_required" BOOLEAN NOT NULL DEFAULT FALSE;
