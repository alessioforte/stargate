"use client";

import { Flex } from "@mantine/core";
import styles from "./shell.module.css";
import UserMenu from "../user-menu/user-menu";

interface Props {
  language: string;
  onLanguageChange: (language: "en" | "it") => void;
  onLogout: () => void;
}

const Header: React.FC<Props> = ({ language, onLanguageChange, onLogout }) => {
  return (
    <header className={styles.header}>
      <div className={styles.header_inner}>
        <Flex align="center">
          <div className={styles.logo}>✨</div>
          <h3>Stargate</h3>
        </Flex>
        <UserMenu
          language={language}
          onLanguageChange={onLanguageChange}
          onLogout={onLogout}
        />
      </div>
    </header>
  );
};

export default Header;
