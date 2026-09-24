import { getTokenExpiry } from "./jwt";
import { getStoredTokens, refreshAccessToken, storeTokens } from "@/lib/oauth";

const REFRESH_BUFFER_SECONDS = 60;

export class TokenManager {
  private refreshPromise: Promise<boolean> | null = null;
  private readonly onRefreshFailure?: () => void;

  constructor(onRefreshFailure?: () => void) {
    this.onRefreshFailure = onRefreshFailure;
  }

  async ensureValidToken(): Promise<boolean> {
    const accessToken = this.getAccessToken();
    if (!accessToken) return false;

    const expiresAt = getTokenExpiry(accessToken);
    const now = Math.floor(Date.now() / 1000);
    if (expiresAt !== null && expiresAt - now > REFRESH_BUFFER_SECONDS) {
      return true;
    }

    return this.refresh();
  }

  async refresh(): Promise<boolean> {
    // Concurrent requests share one refresh because refresh tokens rotate.
    if (this.refreshPromise) return this.refreshPromise;

    this.refreshPromise = this.performRefresh();

    try {
      return await this.refreshPromise;
    } finally {
      this.refreshPromise = null;
    }
  }

  getAccessToken(): string | null {
    return getStoredTokens()?.access_token ?? null;
  }

  private async performRefresh(): Promise<boolean> {
    const tokens = getStoredTokens();
    const refreshToken = tokens?.refresh_token;
    if (!refreshToken) {
      this.onRefreshFailure?.();
      return false;
    }

    try {
      const refreshed = await refreshAccessToken(refreshToken);
      storeTokens({
        ...tokens,
        ...refreshed,
        refresh_token: refreshed.refresh_token ?? refreshToken,
      });
      return true;
    } catch {
      this.onRefreshFailure?.();
      return false;
    }
  }
}
