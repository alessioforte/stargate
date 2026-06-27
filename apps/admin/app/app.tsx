import { createBrowserRouter, RouterProvider } from "react-router";
import AuthCallbackPage from "./routes/auth-callback";
import ErrorPage from "./routes/error";
import Layout from "./routes/layout";
import AdminHomePage from "./routes/home";
import UsersPage from "./routes/users/users";
import GatewayPage from "./routes/gateway";
import APIKeysPage from "./routes/api-keys";
import AdminKeysPage from "./routes/admin-keys";
import OAuthClientsPage from "./routes/oauth-clients";
import OrganizationsPage from "./routes/organizations";
import ServiceAccountsPage from "./routes/service-accounts";

const router = createBrowserRouter([
  {
    path: "/auth/callback",
    element: <AuthCallbackPage />,
  },
  {
    path: "/error",
    element: <ErrorPage />,
  },
  {
    path: "/",
    element: <Layout />,
    children: [
      {
        path: "/",
        element: <AdminHomePage />,
      },
      {
        path: "users",
        element: <UsersPage />,
      },
      {
        path: "gateway",
        element: <GatewayPage />,
      },
      {
        path: "api-keys",
        element: <APIKeysPage />,
      },
      {
        path: "admin-keys",
        element: <AdminKeysPage />,
      },
      {
        path: "oauth-clients",
        element: <OAuthClientsPage />,
      },
      {
        path: "organizations",
        element: <OrganizationsPage />,
      },
      {
        path: "service-accounts",
        element: <ServiceAccountsPage />,
      },
      {
        path: "*",
        element: <AdminHomePage />,
      },
    ],
  },
]);

function App() {
  return <RouterProvider router={router} />;
}

export default App;
