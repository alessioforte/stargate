import { useState } from "react";
import {
  ActionIcon,
  Badge,
  Button,
  Divider,
  Flex,
  Group,
  Modal,
  Stack,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { useTranslations } from "@/i18n";
import { AiOutlineDelete } from "react-icons/ai";
import type { UpdateUserRequest, User, JsonValue } from "@/services/types";
import {
  EditActionControls,
  EntityDrawer,
  JsonAttributesForm,
} from "@/components";

interface FormValues {
  id: string;
  nickname: string;
  email: string;
  givenName: string;
  familyName: string;
}

interface Props {
  user?: User | null;
  opened: boolean;
  onClose: () => void;
  onSave: (id: string, user: UpdateUserRequest) => void;
  onDelete: (id: string) => void;
  onAttrsSave: (id: string, attrs: JsonValue) => void;
}

const emptyFormValues: FormValues = {
  id: "",
  nickname: "",
  email: "",
  givenName: "",
  familyName: "",
};

function userToFormValues(user?: User | null): FormValues {
  if (!user) return emptyFormValues;

  return {
    id: user.id,
    nickname: user.nickname,
    email: user.email,
    givenName: user.givenName ?? "",
    familyName: user.familyName ?? "",
  };
}

function EditUserForm({
  user,
  onDeleteClick,
  onSave,
}: {
  user?: User | null;
  onDeleteClick: () => void;
  onSave: (id: string, user: UpdateUserRequest) => void;
}) {
  const t = useTranslations();
  const [edit, setEdit] = useState(false);

  const form = useForm<FormValues>({
    initialValues: userToFormValues(user),
    validate: {
      nickname: (value) => (value ? undefined : "Nickname is required"),
      email: (value) => (value ? undefined : "Email is required"),
      givenName: (value) => (value ? undefined : "Given name is required"),
      familyName: (value) => (value ? undefined : "Family name is required"),
    },
  });

  const handleSubmit = (values: FormValues) => {
    if (!user) return;

    const payload: UpdateUserRequest = {
      nickname: values.nickname.trim(),
      email: values.email.trim(),
      givenName: values.givenName.trim() || null,
      familyName: values.familyName.trim() || null,
    };

    onSave(user.id, payload);
  };

  return (
    <form onSubmit={form.onSubmit(handleSubmit)}>
      <Group mt="sm" px="sm" justify="space-between" gap="xs" align="center">
        <Tooltip label={t("deleteUser")} position="right">
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
            readOnly={!edit}
            variant="filled"
            label={t("nickname")}
            {...form.getInputProps("nickname")}
          />
          <TextInput
            readOnly={!edit}
            variant="filled"
            label={t("email")}
            {...form.getInputProps("email")}
          />
          <TextInput
            readOnly={!edit}
            variant="filled"
            label={t("givenName")}
            {...form.getInputProps("givenName")}
          />
          <TextInput
            readOnly={!edit}
            variant="filled"
            label={t("familyName")}
            {...form.getInputProps("familyName")}
          />
        </Stack>
      </Flex>
    </form>
  );
}

const EditUser: React.FC<Props> = ({
  user,
  onClose,
  onSave,
  onDelete,
  onAttrsSave,
  opened,
}) => {
  const t = useTranslations();
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);
  const [deleteConfirmation, setDeleteConfirmation] = useState("");
  const [deleteModalOpened, deleteModal] = useDisclosure(false);
  const deleteConfirmed = deleteConfirmation === "delete";

  const handleOpenDeleteModal = () => {
    if (!user) return;

    setDeleteTarget(user);
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
      <EntityDrawer opened={opened} onClose={onClose} title={t("user")}>
        {user && (
          <>
            <EditUserForm
              key={user.id}
              user={user}
              onDeleteClick={handleOpenDeleteModal}
              onSave={(id, user) => {
                onSave(id, user);
                onClose();
              }}
            />

            <Divider my="md" />

            <JsonAttributesForm
              attrs={JSON.stringify(user?.attrs ?? {}, null, 2)}
              onSave={(attrs) => {
                onAttrsSave(user.id, attrs);
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
              {deleteTarget?.email}
            </Badge>
          </Group>
          <Text size="sm">{t("confirmDeleteUser")}</Text>
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
              {t("deleteUser")}
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  );
};

export default EditUser;
