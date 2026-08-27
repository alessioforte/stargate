import { useCallback } from "react";
import { Badge, Code, Group, Stack, Text } from "@mantine/core";
import { EntityDrawer, CodeBox } from "@/components";
import { useTranslations } from "@/i18n";
import type { OutboxEvent } from "@/services/types";
import { formatDate } from "@/lib/format-date";

interface Props {
  event: OutboxEvent | null;
  opened: boolean;
  onClose: () => void;
}

const OutboxEventDetail: React.FC<Props> = ({ event, opened, onClose }) => {
  const t = useTranslations();
  const noop = useCallback(() => {}, []);

  return (
    <EntityDrawer
      opened={opened}
      onClose={onClose}
      title={t("outboxEventDetail")}
      size="xl"
    >
      {event && (
        <>
          <Stack gap={0} p="xs">
            <Text size="xs" c="dimmed">
              {t("eventId")}
            </Text>
            <Code style={{ fontSize: 13 }} block>
              {event.eventId}
            </Code>
          </Stack>

          <Group gap="xs" p="xs">
            <Stack gap={0} style={{ flex: 1 }}>
              <Text size="xs" c="dimmed">
                {t("status")}
              </Text>
              <Badge
                color={event.status === "published" ? "teal" : "yellow"}
                variant="light"
                w="fit-content"
              >
                {event.status}
              </Badge>
            </Stack>
            <Stack gap={0} style={{ flex: 1 }}>
              <Text size="xs" c="dimmed">
                {t("seq")}
              </Text>
              <Text fw={500}>{event.seq}</Text>
            </Stack>
          </Group>

          <Group gap="xs" p="xs">
            <Stack gap={0} style={{ flex: 1 }}>
              <Text size="xs" c="dimmed">
                {t("createdAt")}
              </Text>
              <Text size="sm">{formatDate(event.createdAt)}</Text>
            </Stack>
            <Stack gap={0} style={{ flex: 1 }}>
              <Text size="xs" c="dimmed">
                {t("publishedAt")}
              </Text>
              <Text size="sm">
                {event.publishedAt ? formatDate(event.publishedAt) : "—"}
              </Text>
            </Stack>
          </Group>

          {event.operationId && (
            <Stack gap={0} p="xs">
              <Text size="xs" c="dimmed">
                {t("operationId")}
              </Text>
              <Code style={{ fontSize: 13 }} block>
                {event.operationId}
              </Code>
            </Stack>
          )}

          <Stack gap={0} mt="md">
            <Text size="xs" c="dimmed" mb={4} p="xs">
              {t("payload")}
            </Text>

            <CodeBox
              readOnly
              value={JSON.stringify(event.payload, null, 2)}
              onChange={noop}
            />
          </Stack>
        </>
      )}
    </EntityDrawer>
  );
};

export default OutboxEventDetail;
