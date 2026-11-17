import { Burger } from "@mantine/core";
import type { AdminMe } from "@/services/types";
import ColorSchemeToggle from "../colorscheme-toggle/colorscheme-toggle";
import UserMenu from "../user-menu/user-menu";
import styles from "./shell.module.css";

interface Props {
  adminMe: AdminMe | null;
  current?: string;
  language: string;
  menuLabel: string;
  menuOpened: boolean;
  parent?: string;
  onLanguageChange: (language: "en" | "it") => void;
  onLogout: () => void;
  onMenuToggle: () => void;
}

const Header: React.FC<Props> = ({
  adminMe,
  current,
  language,
  menuLabel,
  menuOpened,
  parent,
  onLanguageChange,
  onLogout,
  onMenuToggle,
}) => {
  return (
    <header className={styles.header}>
      <div className={styles.headerStart}>
        <Burger
          size="sm"
          className={styles.burger}
          opened={menuOpened}
          aria-label={menuLabel}
          onClick={onMenuToggle}
        />
        <div className={styles.breadcrumb}>
          {parent && (
            <>
              <span className={styles.breadcrumbParent}>{parent}</span>
              <span className={styles.breadcrumbSeparator}>/</span>
            </>
          )}
          {current && (
            <span className={styles.breadcrumbCurrent}>{current}</span>
          )}
        </div>
      </div>
      <div className={styles.headerEnd}>
        <ColorSchemeToggle />
        <UserMenu
          adminMe={adminMe}
          language={language}
          onLanguageChange={onLanguageChange}
          onLogout={onLogout}
        />
      </div>
    </header>
  );
};

export default Header;
