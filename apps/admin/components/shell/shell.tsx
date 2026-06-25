import styles from "./shell.module.css";
import Header from "./header";
import Sidebar from "./sidebar";
import Footer from "./footer";

interface Props {
  children: React.ReactNode;
  sidebarItems?: {
    label: string;
    path: string;
    icon: React.ReactNode;
  }[];
}

const Shell: React.FC<Props> = ({ children, sidebarItems = [] }) => {
  return (
    <div className={styles.shell}>
      <Header />
      <div className={styles.body}>
        <Sidebar items={sidebarItems} />
        <main className={styles.main}>{children}</main>
      </div>
      <Footer />
    </div>
  );
};

export default Shell;
