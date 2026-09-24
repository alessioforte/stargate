import axios, { type AxiosInstance, type AxiosRequestConfig } from "axios";
import type { ApiMessageParams } from "./types";
import type { TokenManager } from "./token-manager";

export interface Response<T> {
  error?: boolean;
  message?: string;
  data: T | null;
  headers?: unknown;
  status?: number;
  code?: string;
  type?: string;
  link?: string;
  params?: ApiMessageParams;
}

interface ErrorResponse {
  error?: string;
  message?: string;
  error_description?: string;
  code?: string;
  type?: string;
  link?: string;
  params?: ApiMessageParams;
}

export class Http {
  private readonly client: AxiosInstance;
  private readonly tokenManager: TokenManager;

  constructor(baseURL: string, tokenManager: TokenManager) {
    this.client = axios.create({
      baseURL,
      headers: { "Content-Type": "application/json" },
    });
    this.tokenManager = tokenManager;
  }

  async authRequest<T>(config: AxiosRequestConfig): Promise<Response<T>> {
    try {
      if (!(await this.tokenManager.ensureValidToken())) {
        return {
          error: true,
          data: null,
          message: "Unauthorized",
          status: 401,
        };
      }

      const accessToken = this.tokenManager.getAccessToken();
      const response = await this.requestWithToken<T>(config, accessToken);
      if (!response.error || response.status !== 401) {
        return response;
      }

      // Another request may already have refreshed the rejected token.
      const currentToken = this.tokenManager.getAccessToken();
      if (
        (currentToken && currentToken !== accessToken) ||
        (await this.tokenManager.refresh())
      ) {
        return this.requestWithToken<T>(
          config,
          this.tokenManager.getAccessToken(),
        );
      }

      return response;
    } catch (error: unknown) {
      return this.handleError<T>(error);
    }
  }

  async request<T>(config: AxiosRequestConfig): Promise<Response<T>> {
    try {
      const { data, status, headers } = await this.client.request<T>(config);
      return { data, status, headers };
    } catch (error: unknown) {
      return this.handleError<T>(error);
    }
  }

  private requestWithToken<T>(
    config: AxiosRequestConfig,
    accessToken: string | null,
  ): Promise<Response<T>> {
    return this.request<T>({
      ...config,
      headers: { ...config.headers, Authorization: `Bearer ${accessToken}` },
    });
  }

  private handleError<T>(error: unknown): Response<T> {
    const response = axios.isAxiosError<ErrorResponse>(error)
      ? error.response
      : undefined;
    const data = response?.data;

    return {
      error: true,
      message:
        data?.error_description ??
        data?.message ??
        (error instanceof Error ? error.message : "Request failed"),
      data: null,
      status: response?.status,
      headers: response?.headers,
      code: data?.code ?? data?.error,
      type: data?.type,
      link: data?.link,
      params: data?.params,
    };
  }
}
