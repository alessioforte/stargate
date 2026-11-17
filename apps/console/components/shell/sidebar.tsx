import { ActionIcon, Tooltip } from "@mantine/core";
import { Link } from "react-router";
import { LuPanelLeftClose, LuPanelLeftOpen } from "react-icons/lu";
import { useTranslations } from "@/i18n";
import Logo from "./logo";
import { isActiveItem, type NavigationGroup } from "./navigation";
import styles from "./shell.module.css";

export interface SidebarStatus {
  databaseStatus: string;
  status: string;
  storeStatus: string;
  version: string;
}

interface Props {
  collapsed?: boolean;
  navigation: NavigationGroup[];
  pathname: string;
  status: SidebarStatus | null;
  /** Rendered inside the mobile drawer: always expanded, no collapse control. */
  mobile?: boolean;
  onNavigate?: () => void;
  onToggleCollapsed?: () => void;
}

const statusKeys: Record<string, string> = {
  healthy: "shell.statusHealthy",
  degraded: "shell.statusDegraded",
  unhealthy: "shell.statusUnhealthy",
};

const Sidebar: React.FC<Props> = ({
  collapsed = false,
  navigation,
  pathname,
  status,
  mobile = false,
  onNavigate,
  onToggleCollapsed,
}) => {
  const t = useTranslations();
  const statusTitle = status
    ? t(statusKeys[status.status] ?? "shell.statusUnknown")
    : t("shell.statusConnecting");

  return (
    <aside className={mobile ? styles.sidebarMobile : styles.sidebar}>
      <Link to="/" className={styles.brand} onClick={onNavigate}>
        <span className={styles.brandMark}>
          <Logo />
        </span>
        <span className={styles.brandName}>Stargate</span>
        <span className={styles.brandTag}>Console</span>
      </Link>

      <nav className={styles.nav} aria-label={t("shell.navigation")}>
        {navigation.map((group, index) => (
          <div className={styles.navGroup} key={group.label ?? index}>
            {group.label && (
              <div className={styles.navGroupLabel}>{group.label}</div>
            )}
            {group.items.map((item) => (
              <Tooltip
                key={item.path}
                label={item.label}
                position="right"
                offset={12}
                disabled={!collapsed}
              >
                <Link
                  to={item.path}
                  className={styles.navItem}
                  aria-current={
                    isActiveItem(pathname, item.path) ? "page" : undefined
                  }
                  aria-label={collapsed ? item.label : undefined}
                  onClick={onNavigate}
                >
                  <span className={styles.navIcon}>{item.icon}</span>
                  <span className={styles.navLabel}>{item.label}</span>
                </Link>
              </Tooltip>
            ))}
          </div>
        ))}
      </nav>

      <div className={styles.sidebarFooter}>
        <Tooltip
          label={statusTitle}
          position="right"
          offset={12}
          disabled={!collapsed}
        >
          <div className={styles.status}>
            <span
              className={styles.statusDot}
              data-status={status?.status}
              role="img"
              aria-label={statusTitle}
            />
            <div className={styles.statusText}>
              <div className={styles.statusTitle}>{statusTitle}</div>
              {status && (
                <div className={styles.statusDetail}>v{status.version}</div>
              )}
            </div>
          </div>
        </Tooltip>
        {!mobile && (
          <Tooltip
            label={
              collapsed ? t("shell.expandSidebar") : t("shell.collapseSidebar")
            }
            position="right"
            offset={12}
          >
            <ActionIcon
              color="gray"
              aria-label={
                collapsed
                  ? t("shell.expandSidebar")
                  : t("shell.collapseSidebar")
              }
              onClick={onToggleCollapsed}
            >
              {collapsed ? (
                <LuPanelLeftOpen size={17} />
              ) : (
                <LuPanelLeftClose size={17} />
              )}
            </ActionIcon>
          </Tooltip>
        )}
      </div>
    </aside>
  );
};

export default Sidebar;
