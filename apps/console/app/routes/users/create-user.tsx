import {
  Button,
  Divider,
  TextInput,
  PasswordInput,
  Stack,
  Flex,
  Group,
  Select,
  Text,
  SegmentedControl,
} from "@mantine/core";
import { useState } from "react";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { LuPlus } from "react-icons/lu";
import { useTranslations } from "@/i18n";
import type {
  CreateUserInvitationRequest,
  CreateUserRequest,
  Organization,
} from "@/services/types";
import { CodeBox, EntityDrawer } from "@/components";
import { organizationOptions } from "@/lib/organization-options";

interface FormValues {
  nickname: string;
  email: string;
  givenName: string;
  familyName: string;
  password: string;
  confirmPassword: string;
  phoneNumber: string;
  picture: string;
  locale: string;
  organizationId: string;
  role: string;
  attrs: string;
  clientId: string;
  targetLinkUri: string;
}

interface App {
  id: string;
  name: string;
}

interface Props {
  organizations: Organization[];
  /** Enabled OAuth clients that register an `initiate_login_uri`. */
  apps: App[];
  onSave: (user: CreateUserRequest) => void;
  onInvite: (user: CreateUserInvitationRequest) => void;
  /** False with `SIGNUP_MODE=closed`: only password creation is offered. */
  invitationsEnabled: boolean;
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
  locale: "",
  organizationId: "",
  role: "member",
  attrs: "{}",
  clientId: "",
  targetLinkUri: "",
};

const CreateUser: React.FC<Props> = ({
  organizations,
  apps,
  onSave,
  onInvite,
  invitationsEnabled,
}) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);
  const [selectedMode, setMode] = useState<CreationMode>("password");
  const mode: CreationMode = invitationsEnabled ? selectedMode : "password";

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
    const organizationId = values.organizationId.trim();
    const profile: CreateUserInvitationRequest = {
      nickname: values.nickname.trim() || null,
      email: values.email.trim(),
      givenName: values.givenName.trim() || null,
      familyName: values.familyName.trim() || null,
      locale: values.locale || null,
      phoneNumber: values.phoneNumber.trim() || null,
      picture: values.picture.trim() || null,
      attrs: JSON.parse(values.attrs) ?? {},
      membership: organizationId
        ? {
            organizationId,
            role: values.role,
          }
        : undefined,
    };

    if (mode === "invitation") {
      const invitation: CreateUserInvitationRequest = { ...profile };
      // Without an app the request is the one the Console sent before: no
      // client fields at all.
      if (values.clientId) {
        invitation.clientId = values.clientId;
        const targetLinkUri = values.targetLinkUri.trim();
        if (targetLinkUri) {
          invitation.targetLinkUri = targetLinkUri;
        }
      }
      onInvite(invitation);
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
      <Button
        leftSection={<LuPlus size={16} />}
        onClick={() => {
          open();
        }}
      >
        {t("newUser")}
      </Button>

      <EntityDrawer opened={opened} onClose={handleCancel} title={t("user")}>
        <form style={{ flex: 1 }} onSubmit={form.onSubmit(handleSubmit)}>
          <Flex
            direction="column"
            justify="space-between"
            style={{ height: "100%" }}
          >
            <Group mt="sm" px="sm" justify="flex-end" h={28}>
              <Button type="submit" size="compact-sm">
                {t("save")}
              </Button>
            </Group>

            <Flex direction="column" justify="space-between">
              <Stack p="sm">
                {invitationsEnabled && (
                  <SegmentedControl
                    radius="md"
                    value={mode}
                    onChange={(value) => setMode(value as CreationMode)}
                    data={[
                      { label: t("createWithPassword"), value: "password" },
                      {
                        label: t("sendRegistrationEmail"),
                        value: "invitation",
                      },
                    ]}
                  />
                )}
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
                {mode === "invitation" && (
                  <Select
                    clearable
                    variant="filled"
                    label={t("language")}
                    data={[
                      { label: "English", value: "en" },
                      { label: "Italiano", value: "it" },
                    ]}
                    value={form.values.locale || null}
                    onChange={(value) =>
                      form.setFieldValue("locale", value ?? "")
                    }
                  />
                )}
                {mode === "invitation" && (
                  <>
                    <Select
                      clearable
                      searchable
                      variant="filled"
                      label={t("returnToApp")}
                      description={t("returnToAppDescription")}
                      data={apps.map((app) => ({
                        label: app.name,
                        value: app.id,
                      }))}
                      value={form.values.clientId || null}
                      onChange={(value) => {
                        const clientId = value ?? "";
                        if (clientId !== form.values.clientId) {
                          form.setFieldValue("targetLinkUri", "");
                        }
                        form.setFieldValue("clientId", clientId);
                      }}
                    />
                    {form.values.clientId && (
                      <TextInput
                        variant="filled"
                        label={t("targetLinkUri")}
                        description={t("targetLinkUriDescription")}
                        {...form.getInputProps("targetLinkUri")}
                      />
                    )}
                  </>
                )}
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
                <Select
                  searchable
                  clearable
                  variant="filled"
                  label={t("initialOrganization")}
                  placeholder={t("selectOrganization")}
                  data={organizationOptions(organizations)}
                  value={form.values.organizationId || null}
                  onChange={(value) =>
                    form.setFieldValue("organizationId", value ?? "")
                  }
                />
                {form.values.organizationId && (
                  <Select
                    variant="filled"
                    label={t("role")}
                    data={["owner", "admin", "member"]}
                    value={form.values.role}
                    allowDeselect={false}
                    onChange={(value) =>
                      form.setFieldValue("role", value ?? "member")
                    }
                  />
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
