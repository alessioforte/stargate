import type { ReactNode } from "react";
import { ActionIcon, Button, Group, Stack, Text, Tooltip } from "@mantine/core";
import { AiOutlineDelete } from "react-icons/ai";
import { useConfirmModal } from "@/components";
import { useTranslations } from "@/i18n";

interface Props {
  canDelete?: boolean;
  children: ReactNode;
  error?: string | null;
  selectedName: string;
  onApply: () => void;
  onDelete: (name: string) => void;
}

const GatewayEntityForm: React.FC<Props> = ({
  canDelete = true,
  children,
  error,
  selectedName,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();

  if (!selectedName) {
    return (
      <Stack p="sm">
        <Text c="dimmed">{t("selectOrCreateObject")}</Text>
      </Stack>
    );
  }

  return (
    <>
      {confirmModal}
      <Stack gap="sm" style={{ flex: 1 }}>
        <Group
          p="xs"
          gap="xs"
          align="center"
          justify={canDelete ? "space-between" : "flex-end"}
        >
          {canDelete && (
            <Tooltip label={t("deleteObject")}>
              <ActionIcon
                type="button"
                color="red"
                variant="light"
                onClick={async () => {
                  const confirmed = await confirm({
                    color: "red",
                    confirmLabel: t("deleteObject"),
                    message: t("confirmDeleteObject"),
                    title: t("deleteObject"),
                  });
                  if (!confirmed) return;
                  onDelete(selectedName);
                }}
              >
                <AiOutlineDelete />
              </ActionIcon>
            </Tooltip>
          )}
          <Button
            type="button"
            size="compact-sm"
            color="teal"
            onClick={onApply}
          >
            {t("applyToDraft")}
          </Button>
        </Group>

        {error && (
          <Text size="sm" c="red">
            {error}
          </Text>
        )}

        {children}
      </Stack>
    </>
  );
};

export default GatewayEntityForm;
