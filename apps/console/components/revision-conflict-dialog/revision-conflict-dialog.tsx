import { Alert, Button, Group, Modal, Stack, Text } from "@mantine/core";
import { useTranslations } from "@/i18n";
import { formatDate } from "@/lib/format-date";

type Props = {
  opened: boolean;
  loading: boolean;
  language: string;
  actorEmail?: string;
  error?: string;
  latest?: {
    version: number;
    updatedAt: string;
    updatedBy?: string | null;
  } | null;
  onReload: () => void;
  onOverwrite: () => void;
  onRetry: () => void;
};

export default function RevisionConflictDialog({
  opened,
  loading,
  language,
  actorEmail,
  error,
  latest,
  onReload,
  onOverwrite,
  onRetry,
}: Props) {
  const t = useTranslations();
  return (
    <Modal
      opened={opened}
      onClose={() => {}}
      title={t("revisionConflictTitle")}
      withCloseButton={false}
      closeOnClickOutside={false}
      closeOnEscape={false}
    >
      <Stack>
        <Text>{t("revisionConflictDescription")}</Text>
        {latest && (
          <Stack gap={4}>
            <Text>
              {t("documentVersion")}: {latest.version}
            </Text>
            <Text>
              {t("updatedBy")}:{" "}
              {actorEmail ?? latest.updatedBy ?? t("systemActor")}
            </Text>
            <Text>
              {t("updatedAt")}: {formatDate(latest.updatedAt, null, language)}
            </Text>
          </Stack>
        )}
        {error && <Alert color="red">{error}</Alert>}
        <Group justify="flex-end">
          {error && (
            <Button onClick={onRetry} loading={loading}>
              {t("retry")}
            </Button>
          )}
          <Button
            variant="default"
            onClick={onReload}
            disabled={!latest || loading}
          >
            {t("reload")}
          </Button>
          <Button
            color="orange"
            onClick={onOverwrite}
            loading={loading}
            disabled={!latest}
          >
            {t("overwrite")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
