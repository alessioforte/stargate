#!/usr/bin/env bash
# Registers the conformance suite's static clients on a running Stargate and
# writes the configuration (with the generated client secrets) shared by the
# Basic OP, RP-Initiated Logout OP, and Back-Channel Logout OP plans.
#
# Usage: op-setup.sh <issuer-url> <admin-username> <admin-password> <output.json>
#
# <issuer-url> is Stargate as the suite sees it. Set API_URL when this script
# reaches Stargate under another address (default: <issuer-url>). The admin
# must be able to log in without MFA (run Stargate with MFA_MODE=off).
set -euo pipefail

base_url="${1:?stargate url}"
username="${2:?admin username}"
password="${3:?admin password}"
output="${4:?output path}"
api_url="${API_URL:-$base_url}"

# Must match the suite's alias in the generated config (callback path).
alias="stargate"
suite_url="https://localhost.emobix.co.uk:8443/test/a/${alias}"
redirect_uri="${suite_url}/callback"

access_token="$(
  jq -n --arg username "$username" --arg password "$password" \
    '{username: $username, password: $password}' |
    curl -skf -X POST "${api_url}/account/login" \
      -H 'content-type: application/json' --data-binary @- |
    jq -r '.accessToken'
)"

# First-party so the suite is not asked for consent; PKCE optional because
# OpenID Connect Core does not require it for confidential clients. Prints
# {client_id, client_secret} in the suite's config format.
create_client() {
  local name="$1" auth_method="$2"
  jq -n --arg name "$name" --arg auth_method "$auth_method" --arg suite_url "$suite_url" '{
      name: $name,
      tokenEndpointAuthMethod: $auth_method,
      grantTypes: ["authorization_code", "refresh_token"],
      responseTypes: ["code"],
      redirectUris: [($suite_url + "/callback")],
      postLogoutRedirectUris: [($suite_url + "/post_logout_redirect")],
      backchannelLogoutUri: ($suite_url + "/backchannel_logout"),
      backchannelLogoutSessionRequired: true,
      scopes: ["openid", "email", "profile", "offline_access"],
      attrs: {first_party: true, require_pkce: false}
    }' |
    curl -skf -X POST "${api_url}/admin/oauth/clients" \
      -H "authorization: Bearer ${access_token}" \
      -H 'content-type: application/json' --data-binary @- |
    jq '{client_id: .clientId, client_secret: .clientSecret}'
}

# The plan uses a second client for some tests and a client_secret_post
# client for the client authentication variant.
client="$(create_client "OpenID conformance suite" client_secret_basic)"
client2="$(create_client "OpenID conformance suite (2)" client_secret_basic)"
client_post="$(create_client "OpenID conformance suite (post)" client_secret_post)"

host_match="https://${base_url#https://}"
jq -n \
  --arg alias "$alias" \
  --arg base_url "$base_url" \
  --arg host_match "$host_match" \
  --argjson client "$client" \
  --argjson client2 "$client2" \
  --argjson client_post "$client_post" \
  --arg username "$username" \
  --arg password "$password" '
  # The hosted login page (/auth/login) is prerendered, so its form works in
  # the suite browser, which does not run the app bundle; it posts natively
  # to /oauth/login. Input ids are generated, so match on name.
  def login($screenshot): {
    task: "Login",
    optional: true,
    match: ($host_match + "/auth/login*"),
    commands: (
      (if $screenshot
        then [["wait", "xpath", "//*", 10, "Log in to your account", "update-image-placeholder-optional"]]
        else [] end)
      + [
        ["text", "name", "username", $username],
        ["text", "name", "password", $password],
        ["click", "css", "button[type=submit]"]
      ])
  };
  def authorize_flow($screenshot): [{
    match: ($host_match + "/oauth/authorize*"),
    tasks: [
      login($screenshot),
      {
        task: "Verify Complete",
        match: "*/test/*/callback*",
        commands: [["wait", "id", "submission_complete", 10]]
      }
    ]
  }];
  # Rejected redirect_uri: Stargate answers with an error page (screenshot).
  def error_page: [{
    match: ($host_match + "/oauth/authorize*"),
    tasks: [{
      task: "Expect redirect_uri error page",
      match: ($host_match + "/oauth/authorize*"),
      commands: [["wait", "xpath", "//*", 10, "redirect_uri", "update-image-placeholder"]]
    }]
  }];
  # RP-Initiated Logout. With an id_token_hint for the session, Stargate
  # signs out at once and returns to post_logout_redirect_uri.
  def logout_redirect: {
    match: ($host_match + "/oauth/logout*"),
    tasks: [{
      task: "Verify post-logout redirect",
      match: "*/test/*/post_logout_redirect*",
      commands: [["wait", "id", "complete", 10]]
    }]
  };
  # Without a usable hint the user confirms on the prerendered logout page
  # (its form posts without JavaScript); without post_logout_redirect_uri the
  # signed-out page follows (screenshot).
  def logout_signed_out($confirm): {
    match: ($host_match + "/oauth/logout*"),
    tasks: (
      (if $confirm then [{
        task: "Confirm logout",
        match: ($host_match + "/auth/logout*"),
        commands: [["click", "css", "button[value=logout]"]]
      }] else [] end)
      + [{
        task: "Signed-out page",
        match: ($host_match + "/auth/signed-out*"),
        commands: [["wait", "xpath", "//*", 10, "You have signed out", "update-image-placeholder"]]
      }])
  };
  # Invalid hint or unregistered post_logout_redirect_uri: 400, no redirect.
  def logout_error: {
    match: ($host_match + "/oauth/logout*"),
    tasks: [{
      task: "Expect logout error page",
      match: ($host_match + "/oauth/logout*"),
      commands: [["wait", "xpath", "//*", 10, "invalid_request", "update-image-placeholder"]]
    }]
  };
  {
    alias: $alias,
    description: "Stargate OpenID Provider (CI)",
    server: {discoveryUrl: ($base_url + "/.well-known/openid-configuration")},
    client: $client,
    client2: $client2,
    client_secret_post: $client_post,
    browser: (authorize_flow(false) + [logout_redirect]),
    override: {
      "oidcc-prompt-login": {browser: authorize_flow(true)},
      "oidcc-max-age-1": {browser: authorize_flow(true)},
      "oidcc-ensure-registered-redirect-uri": {browser: error_page},
      "oidcc-ensure-request-object-with-redirect-uri": {browser: error_page},
      "oidcc-rp-initiated-logout-no-params": {browser: (authorize_flow(false) + [logout_signed_out(true)])},
      "oidcc-rp-initiated-logout-only-state": {browser: (authorize_flow(false) + [logout_signed_out(true)])},
      "oidcc-rp-initiated-logout-no-post-logout-redirect-uri": {browser: (authorize_flow(false) + [logout_signed_out(false)])},
      "oidcc-rp-initiated-logout-no-id-token-hint": {browser: (authorize_flow(false) + [logout_error])},
      "oidcc-rp-initiated-logout-bad-post-logout-redirect-uri": {browser: (authorize_flow(false) + [logout_error])},
      "oidcc-rp-initiated-logout-query-added-to-post-logout-redirect-uri": {browser: (authorize_flow(false) + [logout_error])},
      "oidcc-rp-initiated-logout-modified-id-token-hint": {browser: (authorize_flow(false) + [logout_error])},
      "oidcc-rp-initiated-logout-bad-id-token-hint": {browser: (authorize_flow(false) + [logout_error])}
    }
  }' >"$output"

echo "Registered conformance clients $(jq -r .client_id <<<"$client"), $(jq -r .client_id <<<"$client2"), $(jq -r .client_id <<<"$client_post")"
