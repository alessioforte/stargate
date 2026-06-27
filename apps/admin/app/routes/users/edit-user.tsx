import { useState } from "react";
import {
  Box,
  ActionIcon,
  Button,
  Divider,
  TextInput,
  Stack,
  Flex,
  Drawer,
  Group,
  Title,
  Text,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { IoCloseSharp } from "react-icons/io5";
import { FiEdit2 } from "react-icons/fi";
import { useTranslations } from "@/i18n";
import type { UpdateUserRequest, User, JsonValue } from "@/services/types";
import { CodeBox } from "@/components";

interface FormValues {
  id: string;
  nickname: string;
  email: string;
  givenName: string;
  familyName: string;
}

interface Props {
  user: User;
  opened: boolean;
  onClose: () => void;
  onSave: (id: string, user: UpdateUserRequest) => void;
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
  onClose,
  onSave,
}: {
  user?: User | null;
  onClose: () => void;
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
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("user")}</Title>
        <Group gap="xs">
          <Button
            type="button"
            size="compact-sm"
            onClick={onClose}
            variant="transparent"
          >
            <IoCloseSharp />
          </Button>
        </Group>
      </Group>

      <Group mt="sm" px="sm" justify="flex-end" h={28}>
        {edit && (
          <>
            <Button
              type="button"
              size="compact-sm"
              onClick={() => {
                setEdit(false);
                form.reset();
              }}
              color="gray"
            >
              {t("cancel")}
            </Button>
          </>
        )}
        {edit && (
          <>
            <Button type="submit" size="compact-sm" color="teal">
              {t("save")}
            </Button>
          </>
        )}
        {!edit && (
          <ActionIcon
            type="button"
            color="blue"
            variant="light"
            onClick={() => setEdit(true)}
          >
            <FiEdit2 />
          </ActionIcon>
        )}
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

const AttributesForm = ({
  attrs,
  onAttrsSave,
}: {
  attrs: string;
  onAttrsSave: (attrs: JsonValue) => void;
}) => {
  const t = useTranslations();

  const [edit, setEdit] = useState(false);

  const form = useForm<{ attrs: string }>({
    initialValues: { attrs },
  });

  return (
    <form
      style={{ flex: 1 }}
      onSubmit={form.onSubmit((values) =>
        onAttrsSave(JSON.parse(values.attrs)),
      )}
    >
      <Flex h="100%" direction="column">
        <Group p="sm" justify="space-between">
          <Text size="sm" fw={700}>
            {t("attributes")}
          </Text>
          <Group gap="xs" h={28}>
            {edit && (
              <>
                <Button
                  type="button"
                  size="compact-sm"
                  onClick={() => {
                    setEdit(false);
                    form.reset();
                  }}
                  color="gray"
                >
                  {t("cancel")}
                </Button>
              </>
            )}
            {edit && (
              <>
                <Button type="submit" size="compact-sm" color="teal">
                  {t("save")}
                </Button>
              </>
            )}
            {!edit && (
              <ActionIcon
                type="button"
                color="blue"
                variant="light"
                onClick={() => setEdit(true)}
              >
                <FiEdit2 />
              </ActionIcon>
            )}
          </Group>
        </Group>
        <Box style={{ flex: 1, height: "100%" }}>
          <CodeBox
            readOnly={!edit}
            value={form.values.attrs}
            onChange={(value) => form.setFieldValue("attrs", value)}
          />
        </Box>
      </Flex>
    </form>
  );
};

const EditUser: React.FC<Props> = ({
  user,
  onClose,
  onSave,
  onAttrsSave,
  opened,
}) => {
  return (
    <Drawer.Root size="md" opened={opened} onClose={onClose} position="right">
      <Drawer.Overlay />
      <Drawer.Content>
        {user && (
          <Flex direction="column" style={{ height: "100vh" }}>
            <EditUserForm
              key={user.id}
              user={user}
              onClose={onClose}
              onSave={onSave}
            />

            <Divider my="md" />

            <AttributesForm
              attrs={JSON.stringify(user?.attrs ?? {}, null, 2)}
              onAttrsSave={(attrs) => {
                onAttrsSave(user.id, attrs);
                onClose();
              }}
            />
          </Flex>
        )}
      </Drawer.Content>
    </Drawer.Root>
  );
};

export default EditUser;
