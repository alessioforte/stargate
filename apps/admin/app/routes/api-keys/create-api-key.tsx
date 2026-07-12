import { useEffect, useState } from "react";
import {
  ActionIcon,
  Box,
  Button,
  CopyButton,
  Divider,
  Group,
  Modal,
  SegmentedControl,
  Select,
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
import services from "@/services";
import type {
  ApiKeyOwnerType,
  CreateApiKeyRequest,
  CreateApiKeyResponse,
  ServiceAccount,
  User,
} from "@/services/types";
import { AsyncSearchSelect, CodeBox, EntityDrawer } from "@/components";
import type { AsyncSearchSelectOption } from "@/components/async-search-select/async-search-select";

interface FormValues {
  attrs: string;
  label: string;
  ownerId: string;
  ownerType: ApiKeyOwnerType;
  orgId: string;
}

interface Props {
  onSave: (apiKey: CreateApiKeyRequest) => Promise<CreateApiKeyResponse | null>;
}

const emptyFormValues: FormValues = {
  attrs: "{}",
  label: "",
  ownerId: "",
  ownerType: "user",
  orgId: "",
};

const CreateApiKey: React.FC<Props> = ({ onSave }) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);
  const [keyModalOpened, { open: openKeyModal, close: closeKeyModal }] =
    useDisclosure(false);
  const [createdApiKey, setCreatedApiKey] =
    useState<CreateApiKeyResponse | null>(null);

  const form = useForm<FormValues>({
    initialValues: emptyFormValues,
    validate: {
      label: (value) => (value.trim() ? undefined : t("labelRequired")),
      ownerId: (value) => (value ? undefined : t("ownerRequired")),
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

  const handleSubmit = async (values: FormValues) => {
    const payload: CreateApiKeyRequest = {
      label: values.label.trim(),
      userId: values.ownerType === "user" ? values.ownerId : null,
      serviceAccountId:
        values.ownerType === "service_account" ? values.ownerId : null,
      orgId:
        values.ownerType === "user" && values.orgId ? values.orgId : null,
      attrs: JSON.parse(values.attrs) ?? {},
    };

    const apiKey = await onSave(payload);
    if (!apiKey) return;

    setCreatedApiKey(apiKey);
    form.reset();
    close();
    openKeyModal();
  };

  const handleCancel = () => {
    form.reset();
    close();
  };

  const handleKeyModalClose = () => {
    setCreatedApiKey(null);
    closeKeyModal();
  };

  // Org binding options for user keys: the selected user's memberships.
  const [userOrgOptions, setUserOrgOptions] = useState<
    AsyncSearchSelectOption[]
  >([]);
  const ownerId = form.values.ownerId;
  const ownerType = form.values.ownerType;

  useEffect(() => {
    form.setFieldValue("orgId", "");
    setUserOrgOptions([]);

    if (ownerType !== "user" || !ownerId) return;

    let cancelled = false;
    services.admin.getUserOrganizations(ownerId).then((res) => {
      if (cancelled) return;
      setUserOrgOptions(
        (res.data ?? []).map((org) => ({ value: org.id, label: org.name })),
      );
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ownerId, ownerType]);

  const fetchUsers = async (
    query: string,
  ): Promise<AsyncSearchSelectOption[]> => {
    const res = await services.admin.getUsers({
      q: query || undefined,
      limit: 20,
    });
    return (res.data?.data ?? []).map((user: User) => ({
      value: user.id,
      label: `${user.email} (${user.nickname})`,
    }));
  };

  const fetchServiceAccounts = async (
    query: string,
  ): Promise<AsyncSearchSelectOption[]> => {
    const res = await services.admin.getServiceAccounts({
      q: query || undefined,
      limit: 20,
    });
    return (res.data?.data ?? []).map((sa: ServiceAccount) => ({
      value: sa.id,
      label: `${sa.name} (${sa.id})`,
    }));
  };

  return (
    <>
      <Tooltip label={t("newApiKey")} position="left" offset={10}>
        <ActionIcon
          onClick={() => {
            setCreatedApiKey(null);
            open();
          }}
        >
          <FaPlus />
        </ActionIcon>
      </Tooltip>

      <EntityDrawer opened={opened} onClose={handleCancel} title={t("apiKey")}>
        <form
          style={{ flex: 1, overflowY: "auto" }}
          onSubmit={form.onSubmit(handleSubmit)}
        >
          <Stack p="sm">
            <Group justify="flex-end" h={28}>
              <Button type="submit" size="compact-sm" color="teal">
                {t("save")}
              </Button>
            </Group>

            <Stack gap={4}>
              <Text size="sm" fw={500}>
                {t("ownerType")}
              </Text>
              <Box>
                <SegmentedControl
                  radius="md"
                  value={form.values.ownerType}
                  onChange={(value) => {
                    form.setFieldValue("ownerType", value as ApiKeyOwnerType);
                    form.setFieldValue("ownerId", "");
                  }}
                  data={[
                    { value: "user", label: t("user") },
                    { value: "service_account", label: t("serviceAccount") },
                  ]}
                />
              </Box>
            </Stack>

            <AsyncSearchSelect
              label={
                form.values.ownerType === "service_account"
                  ? t("serviceAccount")
                  : t("user")
              }
              placeholder={
                form.values.ownerType === "service_account"
                  ? t("selectServiceAccount")
                  : t("selectUser")
              }
              fetchFn={
                form.values.ownerType === "service_account"
                  ? fetchServiceAccounts
                  : fetchUsers
              }
              value={form.values.ownerId || null}
              error={form.errors.ownerId}
              onChange={(value) => form.setFieldValue("ownerId", value ?? "")}
            />

            {form.values.ownerType === "user" &&
              form.values.ownerId &&
              userOrgOptions.length > 0 && (
                <Select
                  variant="filled"
                  label={t("organizationOptional")}
                  description={t("orgBindingHint")}
                  placeholder={t("selectOrganization")}
                  data={userOrgOptions}
                  value={form.values.orgId || null}
                  clearable
                  onChange={(value) =>
                    form.setFieldValue("orgId", value ?? "")
                  }
                />
              )}

            <TextInput
              variant="filled"
              label={t("label")}
              {...form.getInputProps("label")}
            />
          </Stack>

          <Divider my="md" />

          <Stack gap="xs">
            <Group px="sm" gap="xs">
              <Text size="sm" fw={700}>
                {t("attributes")}
              </Text>
              {form.errors.attrs && <Text c="red">{form.errors.attrs}</Text>}
            </Group>

            <CodeBox
              value={form.values.attrs}
              onChange={(value) => form.setFieldValue("attrs", value)}
            />
          </Stack>
        </form>
      </EntityDrawer>

      <Modal
        centered
        size="lg"
        opened={keyModalOpened}
        onClose={handleKeyModalClose}
        title={t("apiKeyCreated")}
      >
        {createdApiKey && (
          <Stack>
            <Text size="sm" c="dimmed">
              {t("apiKeyOneTimeWarning")}
            </Text>
            <Group align="flex-end" wrap="nowrap">
              <TextInput
                readOnly
                variant="filled"
                label={t("apiKey")}
                value={createdApiKey.apiKey}
                style={{ flex: 1 }}
              />
              <CopyButton value={createdApiKey.apiKey} timeout={3000}>
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

export default CreateApiKey;
