/**
 * TokenManager — hybrid token refresh strategy with deduplication.
 *
 * Uses the shared `jwt.ts` utility for token decoding.
 */

import { getTokenExpiry } from "./jwt";
import {
  getStoredTokens,
  refreshAccessToken,
  storeTokens,
  type OAuthTokenResponse,
} from "@/lib/oauth";

export type TokenData = OAuthTokenResponse;

interface TokenManagerOptions {
  bufferSeconds?: number;
  onRefreshSuccess?: (tokens: TokenData) => void;
  onRefreshFailure?: () => void;
}

export class TokenManager {
  private refreshPromise: Promise<boolean> | null = null;
  private bufferSeconds: number;
  private onRefreshSuccess?: (tokens: TokenData) => void;
  private onRefreshFailure?: () => void;

  constructor(options: TokenManagerOptions = {}) {
    this.bufferSeconds = options.bufferSeconds ?? 60;
    this.onRefreshSuccess = options.onRefreshSuccess;
    this.onRefreshFailure = options.onRefreshFailure;
  }

  /**
   * Proactive check — call before every request.
   * Returns true if the access token is valid (or was successfully refreshed).
   */
  async ensureValidToken(): Promise<boolean> {
    const accessToken = this.getAccessToken();
    if (!accessToken) {
      return false;
    }

    const exp = getTokenExpiry(accessToken);
    if (exp === null) {
      // Malformed token — try to refresh
      return this.refresh();
    }

    const nowSeconds = Math.floor(Date.now() / 1000);
    const isWithinBuffer = exp - nowSeconds <= this.bufferSeconds;

    if (!isWithinBuffer) {
      // Token is still valid and not close to expiry
      return true;
    }

    // Token expired or within buffer — refresh
    return this.refresh();
  }

  /**
   * Deduplicated refresh. If a refresh is already in-flight, subsequent
   * callers will await the same promise instead of firing parallel requests.
   */
  async refresh(): Promise<boolean> {
    if (this.refreshPromise) {
      return this.refreshPromise;
    }

    this.refreshPromise = this._doRefresh();

    try {
      return await this.refreshPromise;
    } finally {
      this.refreshPromise = null;
    }
  }

  /**
   * Reactive handler — call when a 401 response is received.
   * Delegates to `refresh()` which deduplicates concurrent calls.
   */
  async handleUnauthorized(): Promise<boolean> {
    return this.refresh();
  }

  /**
   * Reads the current access_token from localStorage.
   */
  getAccessToken(): string | null {
    try {
      const raw = localStorage.getItem("auth");
      if (!raw) return null;
      const parsed = JSON.parse(raw);
      return parsed?.access_token ?? null;
    } catch {
      return null;
    }
  }

  // ─── Private ────────────────────────────────────────────────────────

  private async _doRefresh(): Promise<boolean> {
    const refreshToken = this.getRefreshToken();
    if (!refreshToken) {
      this.onRefreshFailure?.();
      return false;
    }

    try {
      const data = await refreshAccessToken(refreshToken);
      const tokens = {
        ...(getStoredTokens() ?? {}),
        ...data,
        refresh_token: data.refresh_token ?? refreshToken,
      };

      // Persist new tokens
      try {
        storeTokens(tokens);
      } catch {
        // localStorage might be unavailable (private browsing, quota exceeded)
      }

      this.onRefreshSuccess?.(tokens);
      return true;
    } catch {
      // Network error or unexpected failure
      this.onRefreshFailure?.();
      return false;
    }
  }

  private getRefreshToken(): string | null {
    try {
      const raw = localStorage.getItem("auth");
      if (!raw) return null;
      const parsed = JSON.parse(raw) as TokenData;
      return parsed?.refresh_token ?? null;
    } catch {
      return null;
    }
  }
}
