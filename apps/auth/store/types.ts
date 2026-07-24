import type {
  AccountOrganization,
  LoginMFAResponse,
  LoginRequest,
  LoginResponse,
  OtpChallengeResponse,
  SignupCompleteRequest,
  SignupVerificationResponse,
} from "@/services/types";
import type { PendingPasswordlessChallenge } from "@/lib/auth-flow";

export interface AppMessage {
  type: "info" | "success" | "warning" | "error";
  text: string;
}

export type AuthStatus =
  | "idle"
  | "loading"
  | "mfa_required"
  | "authenticated"
  | "error";

export type AuthActionResult =
  | {
      status: "authenticated";
      tokens: LoginResponse;
    }
  | {
      status: "mfa_required";
      challenge: LoginMFAResponse;
    }
  | {
      status: "error";
      message: string;
    };

export type ChallengeActionResult =
  | {
      status: "challenge_sent";
      challenge: OtpChallengeResponse;
    }
  | {
      status: "error";
      message: string;
    };

export type MessageActionResult =
  | {
      status: "success";
      message: string;
    }
  | {
      status: "error";
      message: string;
    };

export type SignupLoadResult =
  | {
      status: "success";
      signup: SignupVerificationResponse;
    }
  | {
      status: "error";
      message: string;
    };

export type OrganizationsLoadResult =
  | {
      status: "success";
      organizations: AccountOrganization[];
    }
  | {
      status: "error";
      message: string;
    };

export interface State {
  accountOrganizations: AccountOrganization[] | null;
  message: AppMessage | null;
  loading: boolean;
  authStatus: AuthStatus;
  pendingMfa: LoginMFAResponse | null;
  pendingPasswordless: PendingPasswordlessChallenge | null;
  signup: SignupVerificationResponse | null;
  tokens: LoginResponse | null;
  error: string | null;
  organizationError: string | null;
  organizationSwitchingId: string | null;
  language: string;
}

export interface Actions {
  setLanguage: (lang: "en" | "it") => void;
  loadApiMessages: (locale?: "en" | "it") => Promise<void>;
  login: (credentials: LoginRequest) => Promise<AuthActionResult>;
  verifyMFAChallenge: (
    challengeId: string,
    code: string,
  ) => Promise<AuthActionResult>;
  requestPasswordlessLogin: (email: string) => Promise<ChallengeActionResult>;
  verifyPasswordlessLogin: (
    challengeId: string,
    code: string,
  ) => Promise<AuthActionResult>;
  forgotPassword: (email: string) => Promise<MessageActionResult>;
  changePassword: (
    password: string,
    token: string,
  ) => Promise<MessageActionResult>;
  loadSignup: (token: string) => Promise<SignupLoadResult>;
  completeSignup: (
    payload: SignupCompleteRequest,
  ) => Promise<MessageActionResult>;
  loadAccountOrganizations: () => Promise<OrganizationsLoadResult>;
  switchOrganization: (orgId: string) => Promise<MessageActionResult>;
  clearAuthError: () => void;
}
