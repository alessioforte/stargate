import { useState } from "react";
import {
  ActionIcon,
  Box,
  Button,
  CopyButton,
  Divider,
  Group,
  Stack,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDisclosure } from "@mantine/hooks";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";
import type {
  CreateOAuthClientRequest,
  CreateOAuthClientResponse,
} from "@/services/types";
import { CodeBox, EntityDrawer } from "@/components";
import {
  emptyOAuthClientFormValues,
  toCreateOAuthClientRequest,
  validateOAuthClientForm,
  type OAuthClientFormValues,
} from "./form";
import OAuthClientFormFields from "./oauth-client-form-fields";

interface Props {
  onSave: (
    oauthClient: CreateOAuthClientRequest,
  ) => Promise<CreateOAuthClientResponse | null>;
}

const CreateOAuthClient: React.FC<Props> = ({ onSave }) => {
  const t = useTranslations();
  const [opened, { open, close }] = useDisclosure(false);
  const [createdOAuthClient, setCreatedOAuthClient] =
    useState<CreateOAuthClientResponse | null>(null);

  const form = useForm<OAuthClientFormValues>({
    initialValues: emptyOAuthClientFormValues,
    validate: validateOAuthClientForm(t),
  });

  const handleSubmit = async (values: OAuthClientFormValues) => {
    const oauthClient = await onSave(toCreateOAuthClientRequest(values));
    if (!oauthClient) return;

    setCreatedOAuthClient(oauthClient);
    form.reset();
  };

  const handleCancel = () => {
    setCreatedOAuthClient(null);
    form.reset();
    close();
  };

  return (
    <>
      <Tooltip label={t("newOAuthClient")} position="left" offset={10}>
        <ActionIcon
          onClick={() => {
            setCreatedOAuthClient(null);
            open();
          }}
        >
          <FaPlus />
        </ActionIcon>
      </Tooltip>

      <EntityDrawer
        opened={opened}
        onClose={handleCancel}
        title={t("oauthClient")}
      >
        <form onSubmit={form.onSubmit(handleSubmit)}>
          <Group mt="sm" px="sm" justify="flex-end" h={28}>
            <Button type="submit" size="compact-sm" color="teal">
              {t("save")}
            </Button>
          </Group>

          <OAuthClientFormFields form={form} includeClientId />

          <Divider my="md" />

          <Stack>
            <Group p="xs" gap="xs">
              <Text size="sm" fw={700}>
                {t("attributes")}
              </Text>
              {form.errors.attrs && <Text c="red">{form.errors.attrs}</Text>}
            </Group>
            <Box h={220}>
              <CodeBox
                value={form.values.attrs}
                onChange={(value) => form.setFieldValue("attrs", value)}
              />
            </Box>
          </Stack>

          {createdOAuthClient && (
            <>
              <Divider my="md" />
              <Stack px="sm" pb="sm">
                <TextInput
                  readOnly
                  variant="filled"
                  label={t("clientId")}
                  value={createdOAuthClient.clientId}
                />
                {createdOAuthClient.clientSecret && (
                  <Group align="flex-end" wrap="nowrap">
                    <TextInput
                      readOnly
                      variant="filled"
                      label={t("clientSecret")}
                      value={createdOAuthClient.clientSecret}
                      style={{ flex: 1 }}
                    />
                    <CopyButton value={createdOAuthClient.clientSecret}>
                      {({ copied, copy }) => (
                        <Button type="button" onClick={copy} color="blue">
                          {copied ? t("copied") : t("copy")}
                        </Button>
                      )}
                    </CopyButton>
                  </Group>
                )}
              </Stack>
            </>
          )}
        </form>
      </EntityDrawer>
    </>
  );
};

export default CreateOAuthClient;
