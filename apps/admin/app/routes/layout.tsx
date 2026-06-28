import { useEffect } from "react";
import { Outlet, useLocation, useNavigate } from "react-router";
import { Center, Stack, Text } from "@mantine/core";
import { RiHomeLine } from "react-icons/ri";
import { HiOutlineUsers } from "react-icons/hi2";
import { GoOrganization } from "react-icons/go";
import { GrServices } from "react-icons/gr";
import { AiOutlineApi } from "react-icons/ai";
import { GoShieldLock } from "react-icons/go";
import { AiOutlineAppstoreAdd } from "react-icons/ai";
import { GrGateway } from "react-icons/gr";
import { Shell, Loader } from "@/components";
import useStore from "@/store";

function currentReturnPath(pathname: string, query: string) {
  return query ? `${pathname}${query}` : pathname;
}

const sidebarItems = [
  { label: "Home", path: "/", icon: <RiHomeLine size={18} /> },
  {
    label: "Organizations",
    path: "/organizations",
    icon: <GoOrganization size={18} />,
  },
  { label: "Users", path: "/users", icon: <HiOutlineUsers size={18} /> },
  {
    label: "Service Accounts",
    path: "/service-accounts",
    icon: <GrServices size={18} />,
  },
  {
    label: "API Keys",
    path: "/api-keys",
    icon: <AiOutlineApi size={18} />,
  },
  {
    label: "Admin Keys",
    path: "/admin-keys",
    icon: <GoShieldLock size={18} />,
  },
  {
    label: "OAuth Clients",
    path: "/oauth-clients",
    icon: <AiOutlineAppstoreAdd size={18} />,
  },
  {
    label: "Gateway Configurations",
    path: "/gateway",
    icon: <GrGateway size={18} />,
  },
];

const Layout = () => {
  const location = useLocation();
  const navigate = useNavigate();
  const returnPath = currentReturnPath(location.pathname, location.search);
  const adminSessionStatus = useStore((state) => state.adminSessionStatus);
  const ensureAdminSession = useStore((state) => state.ensureAdminSession);
  const loading = useStore((state) => state.loading);

  useEffect(() => {
    let cancelled = false;

    ensureAdminSession(returnPath).then((result) => {
      if (cancelled) return;

      if (result.status === "error") {
        navigate("/error", { replace: true });
      }
    });

    return () => {
      cancelled = true;
    };
  }, [ensureAdminSession, navigate, returnPath]);

  if (adminSessionStatus !== "ready") {
    return (
      <Center mih="100vh">
        <Stack>
          <Loader />
          <Text c="dimmed">
            {adminSessionStatus === "redirecting"
              ? "Redirecting to sign in"
              : "Checking session"}
          </Text>
        </Stack>
      </Center>
    );
  }

  return (
    <Shell sidebarItems={sidebarItems}>
      {loading && <Loader />}
      {!loading && <Outlet />}
    </Shell>
  );
};

export default Layout;
