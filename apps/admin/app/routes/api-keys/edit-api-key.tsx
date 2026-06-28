import { useState } from "react";
import {
  ActionIcon,
  Badge,
  Button,
  Divider,
  Group,
  Modal,
  Stack,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { AiOutlineDelete } from "react-icons/ai";
import { useTranslations } from "@/i18n";
import type { ApiKey, JsonValue } from "@/services/types";
import { EntityDrawer, JsonAttributesForm } from "@/components";

interface Props {
  apiKey?: ApiKey | null;
  opened: boolean;
  onAttrsSave: (id: string, attrs: JsonValue) => void;
  onClose: () => void;
  onDelete: (id: string) => void;
  onRevoke: (id: string) => void;
}

const EditApiKey: React.FC<Props> = ({
  apiKey,
  onAttrsSave,
  onClose,
  onDelete,
  onRevoke,
  opened,
}) => {
  const t = useTranslations();
  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null);
  const [deleteConfirmation, setDeleteConfirmation] = useState("");
  const [deleteModalOpened, deleteModal] = useDisclosure(false);
  const deleteConfirmed = deleteConfirmation === "delete";

  const handleOpenDeleteModal = () => {
    if (!apiKey) return;

    setDeleteTarget(apiKey);
    setDeleteConfirmation("");
    onClose();
    deleteModal.open();
  };

  const handleCloseDeleteModal = () => {
    setDeleteTarget(null);
    setDeleteConfirmation("");
    deleteModal.close();
  };

  const handleConfirmDelete = () => {
    if (!deleteTarget || !deleteConfirmed) return;

    onDelete(deleteTarget.id);
    handleCloseDeleteModal();
  };

  return (
    <>
      <EntityDrawer opened={opened} onClose={onClose} title={t("apiKey")}>
        {apiKey && (
          <>
            <Group mt="sm" px="sm" justify="space-between" gap="xs">
              <Tooltip label={t("deleteApiKey")} position="right">
                <ActionIcon
                  type="button"
                  color="red"
                  variant="light"
                  onClick={handleOpenDeleteModal}
                >
                  <AiOutlineDelete />
                </ActionIcon>
              </Tooltip>
              <Group gap="xs">
                <Badge color={apiKey.revoked ? "red" : "teal"}>
                  {apiKey.revoked ? t("revoked") : t("active")}
                </Badge>
                {!apiKey.revoked && (
                  <Button
                    type="button"
                    color="red"
                    size="compact-sm"
                    onClick={() => {
                      if (!window.confirm(t("confirmRevokeApiKey"))) return;
                      onRevoke(apiKey.id);
                      onClose();
                    }}
                  >
                    {t("revoke")}
                  </Button>
                )}
              </Group>
            </Group>

            <Stack p="sm">
              <TextInput
                readOnly
                variant="filled"
                label={t("id")}
                value={apiKey.id}
              />
              <TextInput
                readOnly
                variant="filled"
                label={t("label")}
                value={apiKey.label}
              />
            </Stack>

            <Divider my="md" />

            <JsonAttributesForm
              attrs={JSON.stringify(apiKey.attrs ?? {}, null, 2)}
              onSave={(attrs) => {
                onAttrsSave(apiKey.id, attrs);
                onClose();
              }}
            />
          </>
        )}
      </EntityDrawer>

      <Modal
        centered
        opened={deleteModalOpened}
        onClose={handleCloseDeleteModal}
        title={t("dangerousZone")}
      >
        <Stack>
          <Group my="md" justify="center">
            <Badge color="red" size="lg" variant="light">
              {deleteTarget?.label}
            </Badge>
          </Group>
          <Text size="sm">{t("confirmDeleteApiKey")}</Text>
          <TextInput
            data-autofocus
            variant="filled"
            label={t("typeDeleteToConfirm")}
            placeholder="delete"
            value={deleteConfirmation}
            onChange={(event) =>
              setDeleteConfirmation(event.currentTarget.value)
            }
          />
          <Group justify="flex-end">
            <Button
              type="button"
              variant="default"
              onClick={handleCloseDeleteModal}
            >
              {t("cancel")}
            </Button>
            <Button
              type="button"
              color="red"
              disabled={!deleteConfirmed}
              onClick={handleConfirmDelete}
            >
              {t("deleteApiKey")}
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  );
};

export default EditApiKey;
