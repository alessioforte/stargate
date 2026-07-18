import { Group, Text } from "@mantine/core";
import useStore from "@/store";
import styles from "./shell.module.css";

const Footer = () => {
  const adminStatus = useStore((state) => state.adminStatus);

  const statusColor =
    adminStatus?.status === "healthy"
      ? "var(--mantine-color-green-6)"
      : adminStatus?.status === "degraded"
        ? "var(--mantine-color-yellow-6)"
        : "var(--mantine-color-red-6)";

  return (
    <div className={styles.footer}>
      <div className={styles.footerInner}>
        {adminStatus ? (
          <Group className={styles.footerItems} justify="space-between">
            <Text size="xs" c="dimmed">
              v{adminStatus.version}
            </Text>

            <Group gap="xs" className={styles.footerItems} justify="flex-end">
              <Text size="xs" c="dimmed">
                db: {adminStatus.databaseStatus}
              </Text>

              <Text size="xs" c="dimmed" className={styles.footerSeparator}>
                |
              </Text>

              <Text size="xs" c="dimmed">
                store: {adminStatus.storeStatus}
              </Text>

              <Text size="xs" c="dimmed" className={styles.footerSeparator}>
                |
              </Text>

              <Group>
                <Text size="xs" c="dimmed">
                  {adminStatus.status}
                </Text>
                <span
                  className={styles.statusDot}
                  style={{ backgroundColor: statusColor }}
                />
              </Group>
            </Group>
          </Group>
        ) : (
          <Text size="xs" c="dimmed">
            Connecting...
          </Text>
        )}
      </div>
    </div>
  );
};

export default Footer;
