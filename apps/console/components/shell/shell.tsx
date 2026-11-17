import { useEffect, useRef } from "react";
import { Drawer } from "@mantine/core";
import { useDisclosure, useLocalStorage } from "@mantine/hooks";
import { useLocation } from "react-router";
import { useTranslations } from "@/i18n";
import type { AdminMe } from "@/services/types";
import Header from "./header";
import { findActive, type NavigationGroup } from "./navigation";
import Sidebar, { type SidebarStatus } from "./sidebar";
import styles from "./shell.module.css";

interface Props {
  adminMe: AdminMe | null;
  adminStatus: SidebarStatus | null;
  children: React.ReactNode;
  language: string;
  navigation?: NavigationGroup[];
  onLanguageChange: (language: "en" | "it") => void;
  onLogout: () => void;
}

const Shell: React.FC<Props> = ({
  adminMe,
  adminStatus,
  children,
  language,
  navigation = [],
  onLanguageChange,
  onLogout,
}) => {
  const t = useTranslations();
  const { pathname } = useLocation();
  const mainRef = useRef<HTMLElement>(null);
  const [collapsed, setCollapsed] = useLocalStorage({
    key: "stargate-console-sidebar-collapsed",
    defaultValue: false,
    getInitialValueInEffect: false,
  });
  const [menuOpened, menu] = useDisclosure(false);
  const active = findActive(navigation, pathname);

  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [pathname]);

  return (
    <div className={styles.shell} data-collapsed={collapsed || undefined}>
      <Sidebar
        collapsed={collapsed}
        navigation={navigation}
        pathname={pathname}
        status={adminStatus}
        onToggleCollapsed={() => setCollapsed((value) => !value)}
      />
      <Drawer
        opened={menuOpened}
        onClose={menu.close}
        size={280}
        padding={0}
        withCloseButton={false}
        hiddenFrom="sm"
      >
        <Sidebar
          mobile
          navigation={navigation}
          pathname={pathname}
          status={adminStatus}
          onNavigate={menu.close}
        />
      </Drawer>
      <div className={styles.column}>
        <Header
          adminMe={adminMe}
          current={active?.item.label}
          parent={active?.group.label}
          language={language}
          menuLabel={t("shell.navigation")}
          menuOpened={menuOpened}
          onLanguageChange={onLanguageChange}
          onLogout={onLogout}
          onMenuToggle={menu.toggle}
        />
        <main className={styles.main} ref={mainRef}>
          {children}
        </main>
      </div>
    </div>
  );
};

export default Shell;
