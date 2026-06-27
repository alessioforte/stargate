import {
  ActionIcon,
  Button,
  Divider,
  TextInput,
  PasswordInput,
  Stack,
  Flex,
  Drawer,
  Group,
  Title,
  Text,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { IoCloseSharp } from "react-icons/io5";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";
import type { CreateUserRequest, JsonValue } from "@/services/types";
import { CodeBox } from "@/components";

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
    },
  });

  const handleSubmit = (values: FormValues) => {
    const payload: CreateUserRequest = {
      nickname: values.nickname.trim(),
      email: values.email.trim(),
      givenName: values.givenName.trim() || null,
      familyName: values.familyName.trim() || null,
      attrs: JSON.parse(values.attrs) ?? {},
      password: values.password.trim(),
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

      <Drawer.Root
        size="md"
        opened={opened}
        onClose={handleCancel}
        position="right"
      >
        <Drawer.Overlay />
        <Drawer.Content>
          <form onSubmit={form.onSubmit(handleSubmit)}>
            <Flex
              direction="column"
              justify="space-between"
              style={{ height: "100vh" }}
            >
              <Group p="xs" justify="space-between">
                <Title order={4}>{t("user")}</Title>
                <Group gap="xs">
                  <Button
                    type="button"
                    size="compact-sm"
                    onClick={handleCancel}
                    variant="transparent"
                  >
                    <IoCloseSharp />
                  </Button>
                </Group>
              </Group>

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

              <Text p="sm" size="sm" fw={700}>
                {t("attributes")}
              </Text>

              <CodeBox
                value={form.values.attrs}
                onChange={(value) => form.setFieldValue("attrs", value)}
              />
            </Flex>
          </form>
        </Drawer.Content>
      </Drawer.Root>
    </>
  );
};

export default CreateUser;
