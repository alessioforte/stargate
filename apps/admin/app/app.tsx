import { createBrowserRouter, RouterProvider } from "react-router";
import { getBasePath } from "@/lib/base-path";
import AuthCallbackPage from "./routes/auth-callback";
import ErrorPage from "./routes/error";
import Layout from "./routes/layout";
import AdminHomePage from "./routes/home/home";
import UsersPage from "./routes/users/users";
import GatewayPage from "./routes/gateway/gateway";
import APIKeysPage from "./routes/api-keys/api-keys";
import AdminKeysPage from "./routes/admin-keys/admin-keys";
import OAuthClientsPage from "./routes/oauth-clients/oauth-clients";
import OrganizationsPage from "./routes/organizations/organizations";
import ServiceAccountsPage from "./routes/service-accounts/service-accounts";
import AccessControlPoliciesPage from "./routes/access-control-policies/access-control-policies";

const router = createBrowserRouter(
  [
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
          path: "access-control-policies",
          element: <AccessControlPoliciesPage />,
        },
        {
          path: "audits",
          element: <>audits</>,
        },
        {
          path: "*",
          element: <AdminHomePage />,
        },
      ],
    },
  ],
  {
    basename: getBasePath(),
  },
);

function App() {
  return <RouterProvider router={router} />;
}

export default App;
