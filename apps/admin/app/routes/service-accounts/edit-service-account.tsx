import { useState } from "react";
import {
  ActionIcon,
  Button,
  Flex,
  Group,
  Modal,
  Select,
  Stack,
  Text,
  Textarea,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { AiOutlineDelete } from "react-icons/ai";
import { useTranslations } from "@/i18n";
import type {
  Organization,
  ServiceAccount,
  UpdateServiceAccountRequest,
} from "@/services/types";
import { EditActionControls, EntityDrawer } from "@/components";
import { organizationOptions } from "@/lib/organization-options";

interface FormValues {
  id: string;
  name: string;
  description: string;
  orgId: string;
}

interface Props {
  organizations: Organization[];
  serviceAccount?: ServiceAccount | null;
  opened: boolean;
  onClose: () => void;
  onDelete: (id: string) => void;
  onSave: (id: string, serviceAccount: UpdateServiceAccountRequest) => void;
}

const emptyFormValues: FormValues = {
  id: "",
  name: "",
  description: "",
  orgId: "",
};

function serviceAccountToFormValues(
  serviceAccount?: ServiceAccount | null,
): FormValues {
  if (!serviceAccount) return emptyFormValues;

  return {
    id: serviceAccount.id,
    name: serviceAccount.name,
    description: serviceAccount.description ?? "",
    orgId: serviceAccount.orgId ?? "",
  };
}

function EditServiceAccountForm({
  organizations,
  serviceAccount,
  onDeleteClick,
  onSave,
}: {
  organizations: Organization[];
  serviceAccount?: ServiceAccount | null;
  onDeleteClick: () => void;
  onSave: (id: string, serviceAccount: UpdateServiceAccountRequest) => void;
}) {
  const t = useTranslations();
  const [edit, setEdit] = useState(false);

  const form = useForm<FormValues>({
    initialValues: serviceAccountToFormValues(serviceAccount),
    validate: {
      name: (value) => (value.trim() ? undefined : t("nameRequired")),
    },
  });

  const handleSubmit = (values: FormValues) => {
    if (!serviceAccount) return;

    const payload: UpdateServiceAccountRequest = {
      name: values.name.trim(),
      description: values.description.trim() || null,
    };

    onSave(serviceAccount.id, payload);
  };

  return (
    <form onSubmit={form.onSubmit(handleSubmit)}>
      <Group mt="sm" px="sm" justify="space-between" gap="xs" align="center">
        <Tooltip label={t("deleteServiceAccount")}>
          <ActionIcon
            type="button"
            color="red"
            variant="light"
            onClick={onDeleteClick}
          >
            <AiOutlineDelete />
          </ActionIcon>
        </Tooltip>
        <EditActionControls
          editing={edit}
          justify="flex-end"
          onCancel={() => {
            setEdit(false);
            form.reset();
          }}
          onEdit={() => setEdit(true)}
        />
      </Group>

      <Flex direction="column" justify="space-between">
        <Stack p="sm">
          <TextInput
            readOnly
            variant="filled"
            label={t("id")}
            {...form.getInputProps("id")}
          />
          <TextInput
            readOnly={!edit}
            variant="filled"
            label={t("name")}
            {...form.getInputProps("name")}
          />
          <Textarea
            readOnly={!edit}
            autosize
            minRows={3}
            variant="filled"
            label={t("description")}
            {...form.getInputProps("description")}
          />
          <Select
            disabled
            searchable
            variant="filled"
            label={t("organization")}
            placeholder={t("selectOrganization")}
            data={organizationOptions(organizations, form.values.orgId)}
            value={form.values.orgId || null}
            onChange={() => undefined}
          />
        </Stack>
      </Flex>
    </form>
  );
}

const EditServiceAccount: React.FC<Props> = ({
  organizations,
  serviceAccount,
  onClose,
  onDelete,
  onSave,
  opened,
}) => {
  const t = useTranslations();
  const [deleteTarget, setDeleteTarget] = useState<ServiceAccount | null>(null);
  const [deleteConfirmation, setDeleteConfirmation] = useState("");
  const [deleteModalOpened, deleteModal] = useDisclosure(false);
  const deleteConfirmed = deleteConfirmation === "delete";

  const handleOpenDeleteModal = () => {
    if (!serviceAccount) return;

    setDeleteTarget(serviceAccount);
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
      <EntityDrawer
        opened={opened}
        onClose={onClose}
        title={t("serviceAccount")}
      >
        {serviceAccount && (
          <EditServiceAccountForm
            key={serviceAccount.id}
            organizations={organizations}
            serviceAccount={serviceAccount}
            onDeleteClick={handleOpenDeleteModal}
            onSave={(id, serviceAccount) => {
              onSave(id, serviceAccount);
              onClose();
            }}
          />
        )}
      </EntityDrawer>

      <Modal
        centered
        opened={deleteModalOpened}
        onClose={handleCloseDeleteModal}
        title={t("dangerousZone")}
      >
        <Stack>
          <Text size="sm">{t("confirmDeleteServiceAccount")}</Text>
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
              {t("deleteServiceAccount")}
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  );
};

export default EditServiceAccount;
