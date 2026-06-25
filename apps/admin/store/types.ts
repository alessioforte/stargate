export interface AppMessage {
  type: "info" | "success" | "warning" | "error";
  text: string;
}

export type AdminSessionStatus = "checking" | "redirecting" | "ready" | "error";

export interface AdminStatus {
  apiName: string;
  databaseStatus: string;
  redisStatus: string;
  status: string;
  version: string;
}

export type AdminSessionResult =
  | { status: "ready" }
  | { status: "redirecting" }
  | { status: "error"; message: string };

export interface State {
  adminError: string | null;
  adminReturnPath: string;
  adminSessionStatus: AdminSessionStatus;
  adminStatus: AdminStatus | null;
  message: AppMessage | null;
  loading: boolean;
  theme: "light" | "dark" | "system";
  language: string;
}

export interface Actions {
  clearAdminError: () => void;
  ensureAdminSession: (returnPath: string) => Promise<AdminSessionResult>;
  logout: (returnPath: string) => Promise<void>;
  signIn: (returnPath: string) => Promise<void>;
  setTheme: (theme: "light" | "dark" | "system") => void;
  setLanguage: (lang: "en" | "it") => void;
}
