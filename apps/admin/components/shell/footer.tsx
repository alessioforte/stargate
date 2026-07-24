import { Group, Text } from "@mantine/core";
import styles from "./shell.module.css";

export interface FooterStatus {
  databaseStatus: string;
  status: string;
  storeStatus: string;
  version: string;
}

interface Props {
  status: FooterStatus | null;
}

const Footer: React.FC<Props> = ({ status }) => {
  const statusColor =
    status?.status === "healthy"
      ? "var(--mantine-color-green-6)"
      : status?.status === "degraded"
        ? "var(--mantine-color-yellow-6)"
        : "var(--mantine-color-red-6)";

  return (
    <div className={styles.footer}>
      <div className={styles.footerInner}>
        {status ? (
          <Group className={styles.footerItems} justify="space-between">
            <Text size="xs" c="dimmed">
              v{status.version}
            </Text>

            <Group gap="xs" className={styles.footerItems} justify="flex-end">
              <Text size="xs" c="dimmed">
                db: {status.databaseStatus}
              </Text>

              <Text size="xs" c="dimmed" className={styles.footerSeparator}>
                |
              </Text>

              <Text size="xs" c="dimmed">
                store: {status.storeStatus}
              </Text>

              <Text size="xs" c="dimmed" className={styles.footerSeparator}>
                |
              </Text>

              <Group>
                <Text size="xs" c="dimmed">
                  {status.status}
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
