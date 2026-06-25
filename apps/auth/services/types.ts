export interface LoginRequest {
  username: string;
  password: string;
}

export type OtpMethod = "email";
export type MfaMode = "off" | "optional" | "required";

export interface LoginResponse {
  access_token: string;
  refresh_token: string;
  token_type: string;
}

export interface LoginMFAResponse {
  challengeId: string;
  expiresAtUnix: number;
  maxAttempts: number;
  method: OtpMethod;
  mfaRequired: boolean;
  ttlSeconds: number;
}

export interface PasswordlessLoginRequest {
  email: string;
}

export interface PasswordlessLoginVerifyRequest {
  challengeId: string;
  code: string;
}

export interface OtpChallengeResponse {
  challengeId: string;
  expiresAtUnix: number;
  maxAttempts: number;
  method: OtpMethod;
  ttlSeconds: number;
}

export interface GeneralResponse {
  message: string;
  code: string;
}

export interface MFAMethodsResponse {
  methods: OtpMethod[];
  mode: MfaMode;
  preferredMethod: OtpMethod | null;
  required: boolean;
}
