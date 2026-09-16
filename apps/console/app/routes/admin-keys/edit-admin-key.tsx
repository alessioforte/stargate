import { useState } from "react";
import {
  Badge,
  Button,
  Divider,
  Flex,
  Group,
  Stack,
  TextInput,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useTranslations } from "@/i18n";
import type {
  AdminKey,
  AdminKeyPermission,
  UpdateAdminKeyPermissionsRequest,
} from "@/services/types";
import {
  EditActionControls,
  EntityDrawer,
  useConfirmModal,
} from "@/components";
import AdminKeyPermissionsInput from "./permissions-input";
import { isAdminKeyPermission } from "./permissions";

interface FormValues {
  id: string;
  label: string;
  permissions: AdminKeyPermission[];
}

interface Props {
  adminKey?: AdminKey | null;
  opened: boolean;
  onClose: () => void;
  onSave: (id: string, adminKey: UpdateAdminKeyPermissionsRequest) => void;
  onRevoke: (id: string) => void;
}

const emptyFormValues: FormValues = {
  id: "",
  label: "",
  permissions: [],
};

function adminKeyToFormValues(adminKey?: AdminKey | null): FormValues {
  if (!adminKey) return emptyFormValues;

  return {
    id: adminKey.id,
    label: adminKey.label ?? "",
    permissions: adminKey.permissions.filter(isAdminKeyPermission),
  };
}

function EditAdminKeyForm({
  adminKey,
  onSave,
}: {
  adminKey?: AdminKey | null;
  onSave: (id: string, adminKey: UpdateAdminKeyPermissionsRequest) => void;
}) {
  const t = useTranslations();
  const [edit, setEdit] = useState(false);

  const form = useForm<FormValues>({
    initialValues: adminKeyToFormValues(adminKey),
    validate: {
      permissions: (value) =>
        value.length > 0 ? undefined : t("permissionsRequired"),
    },
  });

  const handleSubmit = (values: FormValues) => {
    if (!adminKey) return;

    onSave(adminKey.id, {
      permissions: values.permissions,
    });
  };

  return (
    <form onSubmit={form.onSubmit(handleSubmit)}>
      {!adminKey?.revoked && (
        <EditActionControls
          editing={edit}
          mt="sm"
          px="sm"
          justify="flex-end"
          onCancel={() => {
            setEdit(false);
            form.reset();
          }}
          onEdit={() => setEdit(true)}
        />
      )}

      <Flex direction="column" justify="space-between">
        <Stack p="sm">
          <TextInput
            readOnly
            variant="filled"
            label={t("id")}
            {...form.getInputProps("id")}
          />
          <TextInput
            readOnly
            variant="filled"
            label={t("label")}
            {...form.getInputProps("label")}
          />
          <AdminKeyPermissionsInput
            disabled={!edit || adminKey?.revoked}
            value={form.values.permissions}
            error={form.errors.permissions}
            onChange={(permissions) =>
              form.setFieldValue("permissions", permissions)
            }
          />
        </Stack>
      </Flex>
    </form>
  );
}

const EditAdminKey: React.FC<Props> = ({
  adminKey,
  onClose,
  onSave,
  onRevoke,
  opened,
}) => {
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();

  return (
    <>
      {confirmModal}
      <EntityDrawer opened={opened} onClose={onClose} title={t("adminKey")}>
        {adminKey && (
          <>
            <Group px="sm" pt="sm" justify="space-between">
              <Badge color={adminKey.revoked ? "red" : "teal"}>
                {adminKey.revoked ? t("revoked") : t("active")}
              </Badge>
              {!adminKey.revoked && (
                <Button
                  type="button"
                  color="red"
                  size="compact-sm"
                  onClick={async () => {
                    const confirmed = await confirm({
                      color: "red",
                      confirmLabel: t("revoke"),
                      message: t("confirmRevokeAdminKey"),
                      title: t("revoke"),
                    });
                    if (!confirmed) return;
                    onRevoke(adminKey.id);
                    onClose();
                  }}
                >
                  {t("revoke")}
                </Button>
              )}
            </Group>

            <Divider my="sm" />

            <EditAdminKeyForm
              key={adminKey.id}
              adminKey={adminKey}
              onSave={(id, adminKey) => {
                onSave(id, adminKey);
                onClose();
              }}
            />
          </>
        )}
      </EntityDrawer>
    </>
  );
};

export default EditAdminKey;
