"use client";

import { Flex } from "@mantine/core";
import styles from "./shell.module.css";
import UserMenu from "../user-menu/user-menu";

const Header = () => {
  return (
    <header className={styles.header}>
      <div className={styles.header_inner}>
        <Flex align="center">
          <div className={styles.logo}></div>
          <h3>Stargate</h3>
        </Flex>
        <UserMenu />
      </div>
    </header>
  );
};

export default Header;
