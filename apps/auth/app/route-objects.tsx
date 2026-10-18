import { Navigate, type RouteObject } from "react-router";
import AuthLayout from "./routes/auth-layout";
import ChangePasswordPage from "./routes/change-password";
import ConsentPage from "./routes/consent";
import HomeRedirect from "./routes/home-redirect";
import LoginPage from "./routes/login";
import LoginMFAPage from "./routes/login-mfa";
import PasswordlessLoginPage from "./routes/login-passwordless";
import ResetPasswordPage from "./routes/reset-password";
import SelectOrganizationPage from "./routes/select-organization";
import SignupPage from "./routes/signup";

/**
 * Route tree shared by the browser router and the build-time prerender
 * (`app/entry-server.tsx`), so hydration sees the same component tree.
 */
export const routeObjects: RouteObject[] = [
  {
    element: <AuthLayout />,
    children: [
      {
        index: true,
        element: <HomeRedirect />,
      },
      {
        path: "login",
        element: <LoginPage />,
      },
      {
        path: "login/mfa",
        element: <LoginMFAPage />,
      },
      {
        path: "login/passwordless",
        element: <PasswordlessLoginPage />,
      },
      {
        path: "consent",
        element: <ConsentPage />,
      },
      {
        path: "select-organization",
        element: <SelectOrganizationPage />,
      },
      {
        path: "reset-password",
        element: <ResetPasswordPage />,
      },
      {
        path: "signup",
        element: <SignupPage />,
      },
      {
        path: "signup/verify-email",
        element: <SignupPage />,
      },
      {
        path: "change-password",
        element: <ChangePasswordPage />,
      },
      {
        path: "*",
        element: <Navigate to="/login" replace />,
      },
    ],
  },
];
