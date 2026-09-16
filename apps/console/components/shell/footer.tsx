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

const colorMap: Record<string, string> = {
  healthy: "var(--mantine-color-green-6)",
  degraded: "var(--mantine-color-yellow-6)",
  unhealthy: "var(--mantine-color-red-6)",
};

const Footer: React.FC<Props> = ({ status }) => {
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
                db:
              </Text>
              <span
                className={styles.statusDot}
                style={{ backgroundColor: colorMap[status.databaseStatus] }}
              />

              <Text size="xs" c="dimmed" className={styles.footerSeparator}>
                |
              </Text>

              <Text size="xs" c="dimmed">
                store:
              </Text>
              <span
                className={styles.statusDot}
                style={{ backgroundColor: colorMap[status.storeStatus] }}
              />
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
