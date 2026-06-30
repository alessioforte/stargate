/**
 * JWT utilities shared across the services package.
 *
 * This module is a leaf dependency: it imports nothing from this package,
 * so it can be safely used by both `index.ts` and `token-manager.ts`
 * without creating circular dependencies.
 */

/**
 * Decode a JWT payload and return the claims as a parsed object.
 * Returns `null` if the token is malformed.
 */
export function parseJwt(token: string): Record<string, unknown> | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;

    const base64Url = parts[1]!;
    const base64 = base64Url
      .replace(/-/g, "+")
      .replace(/_/g, "/")
      .padEnd(Math.ceil(base64Url.length / 4) * 4, "=");
    const decoded = atob(base64);

    const jsonPayload = decodeURIComponent(
      decoded
        .split("")
        .map(
          (character: string) =>
            `%${`00${character.charCodeAt(0).toString(16)}`.slice(-2)}`,
        )
        .join(""),
    );

    return JSON.parse(jsonPayload);
  } catch {
    return null;
  }
}

/**
 * Extract the `exp` claim from a JWT.
 * Returns the expiry as a unix timestamp (seconds), or `null` if invalid.
 */
export function getTokenExpiry(token: string): number | null {
  const claims = parseJwt(token);
  if (!claims) return null;
  const exp = claims.exp;
  return typeof exp === "number" ? exp : null;
}
