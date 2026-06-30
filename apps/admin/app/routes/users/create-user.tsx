import {
  Box,
  ActionIcon,
  Button,
  Divider,
  TextInput,
  PasswordInput,
  Stack,
  Flex,
  Group,
  Text,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";
import type { CreateUserRequest } from "@/services/types";
import { CodeBox, EntityDrawer } from "@/components";

interface FormValues {
  nickname: string;
  email: string;
  givenName: string;
  familyName: string;
  password: string;
  attrs: string;
}

interface Props {
  onSave: (user: CreateUserRequest) => void;
}

const emptyFormValues: FormValues = {
  nickname: "",
  email: "",
  givenName: "",
  familyName: "",
  password: "",
  attrs: "{}",
};

const CreateUser: React.FC<Props> = ({ onSave }) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);

  const form = useForm<FormValues>({
    initialValues: emptyFormValues,
    validate: {
      nickname: (value) => (value ? undefined : "Nickname is required"),
      email: (value) => (value ? undefined : "Email is required"),
      givenName: (value) => (value ? undefined : "Given name is required"),
      familyName: (value) => (value ? undefined : "Family name is required"),
      password: (value) => (value ? undefined : "Password is required"),
      attrs: (value) => {
        try {
          JSON.parse(value);
          return undefined;
        } catch {
          return t("invalidJson");
        }
      },
    },
  });

  const handleSubmit = (values: FormValues) => {
    const payload: CreateUserRequest = {
      nickname: values.nickname.trim(),
      email: values.email.trim(),
      givenName: values.givenName.trim() || null,
      familyName: values.familyName.trim() || null,
      password: values.password.trim(),
      attrs: JSON.parse(values.attrs) ?? {},
    };

    onSave(payload);
    form.reset();
    close();
  };

  const handleCancel = () => {
    form.reset();
    close();
  };

  return (
    <>
      <Tooltip label={t("newUser")} position="left" offset={10}>
        <ActionIcon
          onClick={() => {
            open();
          }}
        >
          <FaPlus />
        </ActionIcon>
      </Tooltip>

      <EntityDrawer opened={opened} onClose={handleCancel} title={t("user")}>
        <form style={{ flex: 1 }} onSubmit={form.onSubmit(handleSubmit)}>
          <Flex
            direction="column"
            justify="space-between"
            style={{ height: "100%" }}
          >
            <Group mt="sm" px="sm" justify="flex-end" h={28}>
              <Button type="submit" size="compact-sm" color="teal">
                {t("save")}
              </Button>
            </Group>

            <Flex direction="column" justify="space-between">
              <Stack p="sm">
                <TextInput
                  variant="filled"
                  label={t("nickname")}
                  {...form.getInputProps("nickname")}
                />
                <TextInput
                  variant="filled"
                  label={t("email")}
                  {...form.getInputProps("email")}
                />
                <TextInput
                  variant="filled"
                  label={t("givenName")}
                  {...form.getInputProps("givenName")}
                />
                <TextInput
                  variant="filled"
                  label={t("familyName")}
                  {...form.getInputProps("familyName")}
                />
                <PasswordInput
                  variant="filled"
                  label={t("password")}
                  {...form.getInputProps("password")}
                />
              </Stack>
            </Flex>

            <Divider my="md" />

            <Group p="sm" gap="xs">
              <Text size="sm" fw={700}>
                {t("attributes")}
              </Text>
              {form.errors.attrs && <Text c="red">{form.errors.attrs}</Text>}
            </Group>

            <CodeBox
              value={form.values.attrs}
              onChange={(value) => form.setFieldValue("attrs", value)}
            />
          </Flex>
        </form>
      </EntityDrawer>
    </>
  );
};

export default CreateUser;
