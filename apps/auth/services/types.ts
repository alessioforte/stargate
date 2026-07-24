export interface LoginRequest {
  username: string;
  password: string;
}

export type ApiMessageParams = Record<string, string | number | boolean | null>;

export interface ApiMessages {
  errors: Record<string, string>;
  messages: Record<string, string>;
}

export interface ApiMessageCatalog {
  schema: "stargate/i18n/v1";
  locale: string;
  fallbackLocale: string;
  version: string;
  messages: ApiMessages;
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
  params?: ApiMessageParams;
}

export interface SignupVerificationResponse {
  email: string;
  token: string;
  givenName?: string | null;
  familyName?: string | null;
  nickname?: string | null;
  phoneNumber?: string | null;
}

export interface SignupCompleteRequest {
  token: string;
  givenName: string;
  familyName: string;
  nickname: string;
  password: string;
  phoneNumber?: string | null;
}

export interface MFAMethodsResponse {
  methods: OtpMethod[];
  mode: MfaMode;
  preferredMethod: OtpMethod | null;
  required: boolean;
}

/** One of the caller's org memberships (GET /account/organizations). */
export interface AccountOrganization {
  id: string;
  name: string;
  description?: string | null;
  role: string;
  memberSince?: string | null;
  active: boolean;
}

export interface AccountOrganizationsResponse {
  activeOrgId?: string | null;
  organizations: AccountOrganization[];
}

/** Fresh token pair issued by PUT /account/session/organization. */
export interface SwitchOrganizationResponse {
  accessToken: string;
  refreshToken: string;
  tokenType: string;
  orgId?: string | null;
}
