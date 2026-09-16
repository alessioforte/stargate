import { ActionIcon, Tooltip } from "@mantine/core";
import { Link, useLocation } from "react-router";
import { getBasePath } from "@/lib/base-path";
import styles from "./shell.module.css";

interface Props {
  items: {
    label: string;
    path: string;
    icon: React.ReactNode;
  }[];
}

function normalizePath(path: string) {
  const normalized = path.replace(/\/+$/, "");
  return normalized || "/";
}

function pathWithoutBasePath(pathname: string) {
  const path = normalizePath(pathname);
  const basePath = getBasePath();

  if (basePath === "/" || !path.startsWith(basePath)) {
    return path;
  }

  if (path === basePath) {
    return "/";
  }

  return normalizePath(path.slice(basePath.length));
}

function isActiveItem(pathname: string, itemPath: string) {
  const currentPath = pathWithoutBasePath(pathname);
  const targetPath = normalizePath(itemPath);

  if (targetPath === "/") {
    return currentPath === "/";
  }

  return currentPath === targetPath || currentPath.startsWith(`${targetPath}/`);
}

const Sidebar: React.FC<Props> = ({ items }) => {
  const location = useLocation();

  return (
    <div className={styles.sidebar}>
      <div className={styles.sidebar_inner}>
        <div className={styles.sidebar_items}>
          {items.map((item) => {
            const active = isActiveItem(location.pathname, item.path);
            return (
              <Tooltip
                key={item.path}
                label={item.label}
                position="right"
                offset={15}
              >
                <ActionIcon
                  component={Link}
                  to={item.path}
                  size="xl"
                  variant="transparent"
                  aria-current={active ? "page" : undefined}
                  color={active ? "var(--mantine-primary-color-6)" : "dimmed"}
                >
                  {item.icon}
                </ActionIcon>
              </Tooltip>
            );
          })}
        </div>
      </div>
    </div>
  );
};

export default Sidebar;
