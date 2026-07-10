import {
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
  SegmentedControl,
} from "@mantine/core";
import { useState } from "react";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";
import type {
  CreateUserInvitationRequest,
  CreateUserRequest,
} from "@/services/types";
import { CodeBox, EntityDrawer } from "@/components";

interface FormValues {
  nickname: string;
  email: string;
  givenName: string;
  familyName: string;
  password: string;
  confirmPassword: string;
  phoneNumber: string;
  picture: string;
  attrs: string;
}

interface Props {
  onSave: (user: CreateUserRequest) => void;
  onInvite: (user: CreateUserInvitationRequest) => void;
}

type CreationMode = "password" | "invitation";

const emptyFormValues: FormValues = {
  nickname: "",
  email: "",
  givenName: "",
  familyName: "",
  password: "",
  confirmPassword: "",
  phoneNumber: "",
  picture: "",
  attrs: "{}",
};

const CreateUser: React.FC<Props> = ({ onSave, onInvite }) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);
  const [mode, setMode] = useState<CreationMode>("password");

  const form = useForm<FormValues>({
    initialValues: emptyFormValues,
    validate: {
      email: (value) => (value ? undefined : "Email is required"),
      password: (value) =>
        mode === "password" && !value ? t("passwordRequired") : undefined,
      confirmPassword: (value, values) =>
        mode === "password" && value !== values.password
          ? t("passwordMismatch")
          : undefined,
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
    const profile: CreateUserInvitationRequest = {
      nickname: values.nickname.trim() || null,
      email: values.email.trim(),
      givenName: values.givenName.trim() || null,
      familyName: values.familyName.trim() || null,
      phoneNumber: values.phoneNumber.trim() || null,
      picture: values.picture.trim() || null,
      attrs: JSON.parse(values.attrs) ?? {},
    };

    if (mode === "invitation") {
      onInvite(profile);
    } else {
      onSave({ ...profile, password: values.password });
    }

    form.reset();
    setMode("password");
    close();
  };

  const handleCancel = () => {
    form.reset();
    setMode("password");
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
                <SegmentedControl
                  radius="md"
                  value={mode}
                  onChange={(value) => setMode(value as CreationMode)}
                  data={[
                    { label: t("createWithPassword"), value: "password" },
                    { label: t("sendRegistrationEmail"), value: "invitation" },
                  ]}
                />
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
                <TextInput
                  variant="filled"
                  label={t("phoneNumber")}
                  {...form.getInputProps("phoneNumber")}
                />
                <TextInput
                  variant="filled"
                  label={t("picture")}
                  {...form.getInputProps("picture")}
                />
                {mode === "password" && (
                  <>
                    <PasswordInput
                      variant="filled"
                      label={t("password")}
                      {...form.getInputProps("password")}
                    />
                    <PasswordInput
                      variant="filled"
                      label={t("confirmPassword")}
                      {...form.getInputProps("confirmPassword")}
                    />
                  </>
                )}
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
