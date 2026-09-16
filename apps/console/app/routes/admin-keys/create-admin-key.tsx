import { useState } from "react";
import {
  ActionIcon,
  Button,
  CopyButton,
  Group,
  Modal,
  Stack,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { FaPlus } from "react-icons/fa6";
import { GoCheck, GoCopy } from "react-icons/go";
import { useTranslations } from "@/i18n";
import type {
  AdminKeyPermission,
  CreateAdminKeyRequest,
  CreateAdminKeyResponse,
} from "@/services/types";
import { EntityDrawer } from "@/components";
import AdminKeyPermissionsInput from "./permissions-input";

interface FormValues {
  label: string;
  permissions: AdminKeyPermission[];
}

interface Props {
  onSave: (
    adminKey: CreateAdminKeyRequest,
  ) => Promise<CreateAdminKeyResponse | null>;
}

const emptyFormValues: FormValues = {
  label: "",
  permissions: [],
};

const CreateAdminKey: React.FC<Props> = ({ onSave }) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);
  const [keyModalOpened, { open: openKeyModal, close: closeKeyModal }] =
    useDisclosure(false);
  const [createdAdminKey, setCreatedAdminKey] =
    useState<CreateAdminKeyResponse | null>(null);

  const form = useForm<FormValues>({
    initialValues: emptyFormValues,
    validate: {
      permissions: (value) =>
        value.length > 0 ? undefined : t("permissionsRequired"),
    },
  });

  const handleSubmit = async (values: FormValues) => {
    const payload: CreateAdminKeyRequest = {
      label: values.label.trim() || null,
      permissions: values.permissions,
    };

    const adminKey = await onSave(payload);
    if (!adminKey) return;

    setCreatedAdminKey(adminKey);
    form.reset();
    close();
    openKeyModal();
  };

  const handleCancel = () => {
    form.reset();
    close();
  };

  const handleKeyModalClose = () => {
    setCreatedAdminKey(null);
    closeKeyModal();
  };

  return (
    <>
      <Tooltip label={t("newAdminKey")} position="left" offset={10}>
        <ActionIcon
          onClick={() => {
            setCreatedAdminKey(null);
            open();
          }}
        >
          <FaPlus />
        </ActionIcon>
      </Tooltip>

      <EntityDrawer
        opened={opened}
        onClose={handleCancel}
        title={t("adminKey")}
      >
        <form style={{ flex: 1 }} onSubmit={form.onSubmit(handleSubmit)}>
          <Stack p="sm">
            <Group justify="flex-end" h={28}>
              <Button type="submit" size="compact-sm" color="teal">
                {t("save")}
              </Button>
            </Group>

            <TextInput
              variant="filled"
              label={t("label")}
              {...form.getInputProps("label")}
            />
            <AdminKeyPermissionsInput
              value={form.values.permissions}
              error={form.errors.permissions}
              onChange={(permissions) =>
                form.setFieldValue("permissions", permissions)
              }
            />
          </Stack>
        </form>
      </EntityDrawer>

      <Modal
        centered
        size="lg"
        opened={keyModalOpened}
        onClose={handleKeyModalClose}
        title={t("adminKeyCreated")}
      >
        {createdAdminKey && (
          <Stack>
            <Text size="sm" c="dimmed">
              {t("adminKeyOneTimeWarning")}
            </Text>
            <Group align="flex-end" wrap="nowrap">
              <TextInput
                readOnly
                variant="filled"
                label={t("apiKey")}
                value={createdAdminKey.apiKey}
                style={{ flex: 1 }}
              />
              <CopyButton value={createdAdminKey.apiKey} timeout={3000}>
                {({ copied, copy }) => (
                  <Tooltip label={copied ? t("copied") : t("copy")}>
                    <ActionIcon
                      size="lg"
                      type="button"
                      onClick={copy}
                      color={copied ? "green" : "blue"}
                    >
                      {copied ? <GoCheck /> : <GoCopy />}
                    </ActionIcon>
                  </Tooltip>
                )}
              </CopyButton>
            </Group>
          </Stack>
        )}
      </Modal>
    </>
  );
};

export default CreateAdminKey;
