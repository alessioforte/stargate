import { Http } from "./http";
import type {
  LoginRequest,
  LoginMFAResponse,
  LoginResponse,
  GeneralResponse,
  MFAMethodsResponse,
  OtpChallengeResponse,
  SignupCompleteRequest,
  SignupVerificationResponse,
} from "./types";

export default class AuthService {
  private readonly http: Http;
  private readonly baseURL: string;

  constructor(http: Http, baseURL: string) {
    this.http = http;
    this.baseURL = baseURL;
  }

  async login(payload: LoginRequest) {
    const response = await this.http.request<LoginResponse | LoginMFAResponse>({
      method: "POST",
      url: `${this.baseURL}/account/login`,
      data: payload,
    });
    return response;
  }

  async passwordlessLogin(email: string) {
    const response = await this.http.request<OtpChallengeResponse>({
      method: "POST",
      url: `${this.baseURL}/account/login/otp/email`,
      data: { email },
    });
    return response;
  }

  async verifyPasswordlessLogin(challengeId: string, code: string) {
    const response = await this.http.request<LoginResponse>({
      method: "PUT",
      url: `${this.baseURL}/account/login/otp/email`,
      data: { challengeId, code },
    });
    return response;
  }

  async forgotPassword(email: string) {
    const response = await this.http.request<GeneralResponse>({
      method: "POST",
      url: `${this.baseURL}/account/credentials`,
      data: { email },
    });
    return response;
  }

  async changePassword(password: string, token: string) {
    const response = await this.http.request<GeneralResponse>({
      method: "PUT",
      url: `${this.baseURL}/account/credentials`,
      data: { password, token },
    });
    return response;
  }

  async getSignup(token: string) {
    const response = await this.http.request<SignupVerificationResponse>({
      method: "GET",
      url: `${this.baseURL}/signup`,
      params: { token },
    });
    return response;
  }

  async completeSignup(payload: SignupCompleteRequest) {
    const response = await this.http.request<GeneralResponse>({
      method: "PUT",
      url: `${this.baseURL}/signup`,
      data: payload,
    });
    return response;
  }

  async verifyMFAChallenge(challengeId: string, code: string) {
    const response = await this.http.request<LoginResponse>({
      method: "PUT",
      url: `${this.baseURL}/account/login/mfa/challenges/${challengeId}`,
      data: { code },
    });
    return response;
  }

  async getMFAMethods() {
    const response = await this.http.request<MFAMethodsResponse>({
      method: "GET",
      url: `${this.baseURL}/account/mfa/methods`,
    });
    return response;
  }

  async logout() {
    const response = await this.http.request<GeneralResponse>({
      method: "DELETE",
      url: `${this.baseURL}/account/logout`,
    });
    return response;
  }
}
