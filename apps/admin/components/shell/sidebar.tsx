import { ActionIcon, Tooltip } from "@mantine/core";
import { Link } from "react-router";
import styles from "./shell.module.css";

interface Props {
  items: {
    label: string;
    path: string;
    icon: React.ReactNode;
  }[];
}

const Sidebar: React.FC<Props> = ({ items }) => {
  return (
    <div className={styles.sidebar}>
      <div className={styles.sidebar_inner}>
        <div className={styles.sidebar_items}>
          {items.map((item) => (
            <Tooltip
              key={item.path}
              label={item.label}
              position="right"
              offset={15}
            >
              <Link to={item.path}>
                <ActionIcon
                  size="xl"
                  variant="transparent"
                  className={styles.sidebar_item}
                  c={
                    item.path === window.location.pathname
                      ? "yellow"
                      : undefined
                  }
                >
                  {item.icon}
                </ActionIcon>
              </Link>
            </Tooltip>
          ))}
        </div>
      </div>
    </div>
  );
};

export default Sidebar;
