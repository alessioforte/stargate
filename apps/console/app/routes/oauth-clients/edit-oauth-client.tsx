import { useEffect, useState } from "react";
import {
  Badge,
  Box,
  Button,
  CopyButton,
  Divider,
  Flex,
  Group,
  Stack,
  TextInput,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { useTranslations } from "@/i18n";
import {
  OAuthTokenEndpointAuthMethod,
  type JsonValue,
  type OAuthClient,
  type RotateOAuthClientSecretResponse,
  type UpdateOAuthClientRequest,
} from "@/services/types";
import {
  EditActionControls,
  EntityDrawer,
  JsonAttributesForm,
  useConfirmModal,
} from "@/components";
import {
  oauthClientToFormValues,
  toUpdateOAuthClientRequest,
  validateOAuthClientForm,
  type OAuthClientFormValues,
} from "./form";
import OAuthClientFormFields from "./oauth-client-form-fields";

interface Props {
  oauthClient?: OAuthClient | null;
  opened: boolean;
  onClose: () => void;
  onDelete: (clientId: string) => Promise<void>;
  onDisable: (clientId: string) => Promise<void>;
  onEnable: (clientId: string) => Promise<void>;
  onRotateSecret: (
    clientId: string,
  ) => Promise<RotateOAuthClientSecretResponse | null>;
  onSave: (
    clientId: string,
    oauthClient: UpdateOAuthClientRequest,
  ) => Promise<void>;
}

function clientIsConfidential(oauthClient: OAuthClient) {
  return (
    oauthClient.tokenEndpointAuthMethod !== OAuthTokenEndpointAuthMethod.None
  );
}

function EditOAuthClientForm({
  oauthClient,
  onSave,
}: {
  oauthClient?: OAuthClient | null;
  onSave: (
    clientId: string,
    oauthClient: UpdateOAuthClientRequest,
  ) => Promise<void>;
}) {
  const t = useTranslations();
  const [edit, setEdit] = useState(false);

  const form = useForm<OAuthClientFormValues>({
    initialValues: oauthClientToFormValues(oauthClient),
    validate: validateOAuthClientForm(t),
  });

  const handleSubmit = (values: OAuthClientFormValues) => {
    if (!oauthClient) return;

    onSave(
      oauthClient.clientId,
      toUpdateOAuthClientRequest(values, oauthClient.attrs ?? {}),
    );
  };

  return (
    <form onSubmit={form.onSubmit(handleSubmit)}>
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

      <Flex direction="column" justify="space-between">
        <OAuthClientFormFields disabled={!edit} form={form} includeClientId />
      </Flex>
    </form>
  );
}

const EditOAuthClient: React.FC<Props> = ({
  oauthClient,
  onClose,
  onDelete,
  onDisable,
  onEnable,
  onRotateSecret,
  onSave,
  opened,
}) => {
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();
  const [rotatedSecret, setRotatedSecret] = useState<string | null>(null);

  useEffect(() => {
    setRotatedSecret(null);
  }, [oauthClient?.clientId]);

  const handleAttrsSave = async (attrs: JsonValue) => {
    if (!oauthClient) return;

    await onSave(
      oauthClient.clientId,
      toUpdateOAuthClientRequest(oauthClientToFormValues(oauthClient), attrs),
    );
    onClose();
  };

  const handleRotateSecret = async () => {
    if (!oauthClient) return;
    const confirmed = await confirm({
      color: "cyan",
      confirmLabel: t("rotateSecret"),
      message: t("confirmRotateOAuthClientSecret"),
      title: t("rotateSecret"),
    });
    if (!confirmed) return;

    const response = await onRotateSecret(oauthClient.clientId);
    if (response?.clientSecret) {
      setRotatedSecret(response.clientSecret);
    }
  };

  return (
    <>
      {confirmModal}
      <EntityDrawer opened={opened} onClose={onClose} title={t("oauthClient")}>
        {oauthClient && (
          <>
            <Group px="sm" pt="sm" justify="space-between">
              <Badge color={oauthClient.enabled ? "teal" : "red"}>
                {oauthClient.enabled ? t("enabled") : t("disabled")}
              </Badge>
              <Group gap="xs">
                {clientIsConfidential(oauthClient) && (
                  <Button
                    type="button"
                    color="cyan"
                    size="compact-sm"
                    onClick={handleRotateSecret}
                  >
                    {t("rotateSecret")}
                  </Button>
                )}
                {oauthClient.enabled ? (
                  <Button
                    type="button"
                    color="red"
                    size="compact-sm"
                    disabled={oauthClient.clientId === "stargate_console"}
                    onClick={() => {
                      onDisable(oauthClient.clientId);
                      onClose();
                    }}
                  >
                    {t("disable")}
                  </Button>
                ) : (
                  <Button
                    type="button"
                    color="teal"
                    size="compact-sm"
                    onClick={() => {
                      onEnable(oauthClient.clientId);
                      onClose();
                    }}
                  >
                    {t("enable")}
                  </Button>
                )}
                <Button
                  type="button"
                  color="red"
                  size="compact-sm"
                  disabled={oauthClient.clientId === "stargate_console"}
                  onClick={async () => {
                    const confirmed = await confirm({
                      color: "red",
                      confirmLabel: t("delete"),
                      message: t("confirmDeleteOAuthClient"),
                      title: t("delete"),
                    });
                    if (!confirmed) return;
                    onDelete(oauthClient.clientId);
                    onClose();
                  }}
                >
                  {t("delete")}
                </Button>
              </Group>
            </Group>

            {rotatedSecret && (
              <Stack px="sm" pt="sm">
                <Group align="flex-end" wrap="nowrap">
                  <TextInput
                    readOnly
                    variant="filled"
                    label={t("clientSecret")}
                    value={rotatedSecret}
                    style={{ flex: 1 }}
                  />
                  <CopyButton value={rotatedSecret}>
                    {({ copied, copy }) => (
                      <Button type="button" onClick={copy} color="blue">
                        {copied ? t("copied") : t("copy")}
                      </Button>
                    )}
                  </CopyButton>
                </Group>
              </Stack>
            )}

            <Divider my="md" />

            <EditOAuthClientForm
              key={oauthClient.clientId}
              oauthClient={oauthClient}
              onSave={async (clientId, oauthClient) => {
                await onSave(clientId, oauthClient);
                onClose();
              }}
            />

            <Divider my="md" />

            <Box h={300}>
              <JsonAttributesForm
                attrs={JSON.stringify(oauthClient.attrs ?? {}, null, 2)}
                onSave={handleAttrsSave}
              />
            </Box>
          </>
        )}
      </EntityDrawer>
    </>
  );
};

export default EditOAuthClient;
