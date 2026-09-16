import { ScrollArea } from "@mantine/core";
import styles from "./shell.module.css";
import Header from "./header";
import Sidebar from "./sidebar";
import Footer, { type FooterStatus } from "./footer";
import type { AdminMe } from "@/services/types";

interface Props {
  adminMe: AdminMe | null;
  adminStatus: FooterStatus | null;
  children: React.ReactNode;
  language: string;
  onLanguageChange: (language: "en" | "it") => void;
  onLogout: () => void;
  sidebarItems?: {
    label: string;
    path: string;
    icon: React.ReactNode;
  }[];
}

const Shell: React.FC<Props> = ({
  adminMe,
  adminStatus,
  children,
  language,
  onLanguageChange,
  onLogout,
  sidebarItems = [],
}) => {
  return (
    <div className={styles.shell}>
      <Header
        adminMe={adminMe}
        language={language}
        onLanguageChange={onLanguageChange}
        onLogout={onLogout}
      />
      <div className={styles.body}>
        <Sidebar items={sidebarItems} />
        <main className={styles.main}>
          <ScrollArea h="100%">{children}</ScrollArea>
        </main>
      </div>
      <Footer status={adminStatus} />
    </div>
  );
};

export default Shell;
