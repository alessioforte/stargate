import axios, { type AxiosRequestConfig, type AxiosError } from "axios";
import type { ApiMessageParams } from "./types";

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
  // private tokenManager: TokenManager | null = null;
  private apiKey: string;
  private orgId: string | null;
  private readonly baseURL: string;

  constructor(apiKey: string, orgId: string | null = null, baseURL: string) {
    this.apiKey = apiKey;
    this.orgId = orgId;
    this.baseURL = baseURL;
  }

  public async authRequest<T>(
    config: AxiosRequestConfig,
  ): Promise<Response<T | null>> {
    try {
      const response = await axios({
        baseURL: this.baseURL,
        withCredentials: true,
        ...config,
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.apiKey}`,
          ...(this.orgId ? { "X-Org-Context": this.orgId } : {}),
          ...config.headers,
        },
      });
      return response;
    } catch (err: unknown) {
      const axiosError = err as AxiosError;

      return this.handleError(axiosError);
    }
  }

  async request<T>(config: AxiosRequestConfig): Promise<Response<T | null>> {
    try {
      return await axios({
        baseURL: this.baseURL,
        withCredentials: true,
        headers: {
          "Content-Type": "application/json",
          ...config.headers,
        },
        ...config,
      });
    } catch (err: unknown) {
      return this.handleError<T>(err as AxiosError);
    }
  }

  private handleError<T>(err: AxiosError): Response<T> {
    const data = err.response?.data as ErrorResponse;
    const message = data?.error_description ?? data?.message ?? err.message;
    return {
      error: true,
      message,
      data: null,
      status: err.response?.status,
      headers: err.response?.headers || null,
      code: data?.code ?? data?.error,
      type: data?.type,
      link: data?.link,
      params: data?.params,
    };
  }

  public setApiKey(apiKey: string) {
    this.apiKey = apiKey;
  }

  public getApiKey(): string {
    return this.apiKey;
  }

  public setOrgId(orgId: string | null) {
    this.orgId = orgId;
  }

  public getOrgId(): string | null {
    return this.orgId;
  }
}
