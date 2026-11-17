import { useEffect, useMemo } from "react";
import { Outlet, useLocation, useNavigate } from "react-router";
import { Center, Stack, Text } from "@mantine/core";
import {
  LuAppWindow,
  LuBot,
  LuBuilding2,
  LuKeyRound,
  LuLayoutDashboard,
  LuMonitorSmartphone,
  LuRoute,
  LuScale,
  LuScrollText,
  LuShieldCheck,
  LuUsers,
} from "react-icons/lu";
import { Shell, Loader, type NavigationGroup } from "@/components";
import { TranslationProvider, useTranslations } from "@/i18n";
import useStore from "@/store";

function currentReturnPath(pathname: string, query: string) {
  return query ? `${pathname}${query}` : pathname;
}

const ICON_SIZE = 17;

const navigationGroups = [
  {
    items: [
      {
        labelKey: "shell.nav.overview",
        path: "/",
        icon: <LuLayoutDashboard size={ICON_SIZE} />,
      },
    ],
  },
  {
    labelKey: "shell.groups.identity",
    items: [
      {
        labelKey: "organizations",
        path: "/organizations",
        icon: <LuBuilding2 size={ICON_SIZE} />,
      },
      { labelKey: "users", path: "/users", icon: <LuUsers size={ICON_SIZE} /> },
      {
        labelKey: "sessions",
        path: "/sessions",
        icon: <LuMonitorSmartphone size={ICON_SIZE} />,
      },
      {
        labelKey: "serviceAccounts",
        path: "/service-accounts",
        icon: <LuBot size={ICON_SIZE} />,
      },
    ],
  },
  {
    labelKey: "shell.groups.credentials",
    items: [
      {
        labelKey: "apiKeys",
        path: "/api-keys",
        icon: <LuKeyRound size={ICON_SIZE} />,
      },
      {
        labelKey: "adminKeys",
        path: "/admin-keys",
        icon: <LuShieldCheck size={ICON_SIZE} />,
      },
      {
        labelKey: "oauthClients",
        path: "/oauth-clients",
        icon: <LuAppWindow size={ICON_SIZE} />,
      },
    ],
  },
  {
    labelKey: "shell.groups.gateway",
    items: [
      {
        labelKey: "shell.nav.configuration",
        path: "/gateway",
        icon: <LuRoute size={ICON_SIZE} />,
      },
      {
        labelKey: "shell.nav.accessControl",
        path: "/access-control-policies",
        icon: <LuScale size={ICON_SIZE} />,
      },
    ],
  },
  {
    labelKey: "shell.groups.monitoring",
    items: [
      {
        labelKey: "audits",
        path: "/audits",
        icon: <LuScrollText size={ICON_SIZE} />,
      },
    ],
  },
];

type ShellProps = Omit<React.ComponentProps<typeof Shell>, "navigation">;

/** Resolves the navigation labels inside the translation provider. */
const ConsoleShell: React.FC<ShellProps> = (props) => {
  const t = useTranslations();
  const navigation = useMemo<NavigationGroup[]>(
    () =>
      navigationGroups.map((group) => ({
        label: group.labelKey ? t(group.labelKey) : undefined,
        items: group.items.map(({ labelKey, ...item }) => ({
          ...item,
          label: t(labelKey),
        })),
      })),
    [t],
  );

  return <Shell {...props} navigation={navigation} />;
};

const Layout = () => {
  const location = useLocation();
  const navigate = useNavigate();
  const returnPath = currentReturnPath(location.pathname, location.search);

  const adminMe = useStore((state) => state.adminMe);
  const adminSessionStatus = useStore((state) => state.adminSessionStatus);
  const adminStatus = useStore((state) => state.adminStatus);
  const ensureAdminSession = useStore((state) => state.ensureAdminSession);
  const getAdminHealth = useStore((state) => state.getAdminHealth);
  const language = useStore((state) => state.language);
  const loadApiMessages = useStore((state) => state.loadApiMessages);
  const loading = useStore((state) => state.loading);
  const logout = useStore((state) => state.logout);
  const setLanguage = useStore((state) => state.setLanguage);

  useEffect(() => {
    void loadApiMessages(language === "it" ? "it" : "en");
  }, [language, loadApiMessages]);

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

  useEffect(() => {
    if (adminSessionStatus !== "ready") return;

    void getAdminHealth();
    const interval = window.setInterval(() => {
      void getAdminHealth();
    }, 30_000);

    return () => window.clearInterval(interval);
  }, [adminSessionStatus, getAdminHealth]);

  if (adminSessionStatus !== "ready") {
    return (
      <Center mih="100vh">
        <Stack align="center">
          <Loader />
          <Text c="dimmed" size="sm">
            {adminSessionStatus === "redirecting"
              ? "Redirecting to sign in"
              : "Checking session"}
          </Text>
        </Stack>
      </Center>
    );
  }

  return (
    <TranslationProvider language={language}>
      <ConsoleShell
        adminMe={adminMe}
        adminStatus={adminStatus}
        language={language}
        onLanguageChange={setLanguage}
        onLogout={() => void logout(returnPath)}
      >
        {loading && <Loader />}
        {!loading && <Outlet />}
      </ConsoleShell>
    </TranslationProvider>
  );
};

export default Layout;
