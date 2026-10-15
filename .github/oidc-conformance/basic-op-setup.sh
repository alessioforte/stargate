#!/usr/bin/env bash
# Registers the conformance suite's static client on a running Stargate and
# writes the Basic OP plan configuration (with the generated client secret).
#
# Usage: basic-op-setup.sh <issuer-url> <admin-username> <admin-password> <output.json>
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
redirect_uri="https://localhost.emobix.co.uk:8443/test/a/${alias}/callback"

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
  jq -n --arg name "$name" --arg auth_method "$auth_method" --arg redirect_uri "$redirect_uri" '{
      name: $name,
      tokenEndpointAuthMethod: $auth_method,
      grantTypes: ["authorization_code", "refresh_token"],
      responseTypes: ["code"],
      redirectUris: [$redirect_uri],
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
  {
    alias: $alias,
    description: "Stargate OpenID Provider (CI)",
    server: {discoveryUrl: ($base_url + "/.well-known/openid-configuration")},
    client: $client,
    client2: $client2,
    client_secret_post: $client_post,
    browser: authorize_flow(false),
    override: {
      "oidcc-prompt-login": {browser: authorize_flow(true)},
      "oidcc-max-age-1": {browser: authorize_flow(true)},
      "oidcc-ensure-registered-redirect-uri": {browser: error_page},
      "oidcc-ensure-request-object-with-redirect-uri": {browser: error_page}
    }
  }' >"$output"

echo "Registered conformance clients $(jq -r .client_id <<<"$client"), $(jq -r .client_id <<<"$client2"), $(jq -r .client_id <<<"$client_post")"
