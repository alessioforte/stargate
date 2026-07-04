import {
  createBrowserRouter,
  Navigate,
  RouterProvider,
  useLocation,
} from "react-router";
import AuthLayout from "./routes/auth-layout";
import ChangePasswordPage from "./routes/change-password";
import LoginPage from "./routes/login";
import LoginMFAPage from "./routes/login-mfa";
import PasswordlessLoginPage from "./routes/login-passwordless";
import ResetPasswordPage from "./routes/reset-password";
import { getAuthBasePath } from "@/lib/base-path";
import "./globals.css";

function HomeRedirect() {
  const location = useLocation();

  return <Navigate to={`/login${location.search}`} replace />;
}

const router = createBrowserRouter(
  [
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
          path: "reset-password",
          element: <ResetPasswordPage />,
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
  ],
  {
    basename: getAuthBasePath(),
  },
);

function App() {
  return <RouterProvider router={router} />;
}

export default App;
