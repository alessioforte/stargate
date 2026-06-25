import type {
  LoginMFAResponse,
  LoginRequest,
  LoginResponse,
  OtpChallengeResponse,
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

export interface State {
  message: AppMessage | null;
  loading: boolean;
  authStatus: AuthStatus;
  pendingMfa: LoginMFAResponse | null;
  pendingPasswordless: PendingPasswordlessChallenge | null;
  tokens: LoginResponse | null;
  error: string | null;
  theme: "light" | "dark" | "system";
  language: string;
}

export interface Actions {
  setTheme: (theme: "light" | "dark" | "system") => void;
  setLanguage: (lang: "en" | "it") => void;
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
  clearAuthError: () => void;
}
