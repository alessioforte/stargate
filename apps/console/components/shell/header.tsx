"use client";

import { Flex } from "@mantine/core";
import styles from "./shell.module.css";
import UserMenu from "../user-menu/user-menu";
import type { AdminMe } from "@/services/types";

interface Props {
  adminMe: AdminMe | null;
  language: string;
  onLanguageChange: (language: "en" | "it") => void;
  onLogout: () => void;
}

const Header: React.FC<Props> = ({
  adminMe,
  language,
  onLanguageChange,
  onLogout,
}) => {
  return (
    <header className={styles.header}>
      <div className={styles.header_inner}>
        <Flex align="center">
          <div className={styles.logo}>✨</div>
          <h3>Stargate</h3>
        </Flex>
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
