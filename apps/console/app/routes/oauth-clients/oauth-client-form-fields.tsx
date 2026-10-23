import {
  Checkbox,
  Select,
  Stack,
  TagsInput,
  TextInput,
  Textarea,
} from "@mantine/core";
import type { UseFormReturnType } from "@mantine/form";
import { useTranslations } from "@/i18n";
import {
  OAuthTokenEndpointAuthMethod,
  type OAuthGrantType,
  type OAuthResponseType,
} from "@/services/types";
import type { OAuthClientFormValues } from "./form";
import {
  oauthGrantTypeTranslationKeys,
  oauthGrantTypeValues,
  oauthResponseTypeTranslationKeys,
  oauthResponseTypeValues,
  oauthTokenEndpointAuthMethodTranslationKeys,
  oauthTokenEndpointAuthMethodValues,
} from "./options";

interface Props {
  disabled?: boolean;
  form: UseFormReturnType<OAuthClientFormValues>;
  includeClientId?: boolean;
}

const OAuthClientFormFields: React.FC<Props> = ({
  disabled = false,
  form,
  includeClientId = false,
}) => {
  const t = useTranslations();

  return (
    <Stack p="sm">
      {includeClientId && (
        <TextInput
          readOnly={disabled}
          variant="filled"
          label={t("clientId")}
          {...form.getInputProps("clientId")}
        />
      )}
      <TextInput
        readOnly={disabled}
        variant="filled"
        label={t("name")}
        {...form.getInputProps("name")}
      />
      <Textarea
        readOnly={disabled}
        autosize
        minRows={3}
        variant="filled"
        label={t("description")}
        {...form.getInputProps("description")}
      />
      <Select
        disabled={disabled}
        allowDeselect={false}
        variant="filled"
        label={t("tokenEndpointAuthMethod")}
        data={oauthTokenEndpointAuthMethodValues.map((method) => ({
          value: method,
          label: t(oauthTokenEndpointAuthMethodTranslationKeys[method]),
        }))}
        {...form.getInputProps("tokenEndpointAuthMethod")}
      />
      {form.values.tokenEndpointAuthMethod ===
        OAuthTokenEndpointAuthMethod.PrivateKeyJwt && (
        <>
          <Textarea
            readOnly={disabled}
            autosize
            minRows={4}
            variant="filled"
            label={t("jwks")}
            description={t("jwksDescription")}
            styles={{ input: { fontFamily: "monospace" } }}
            {...form.getInputProps("jwks")}
          />
          <TextInput
            readOnly={disabled}
            variant="filled"
            label={t("jwksUri")}
            description={t("jwksUriDescription")}
            {...form.getInputProps("jwksUri")}
          />
        </>
      )}
      <Checkbox.Group
        label={t("grantTypes")}
        value={form.values.grantTypes}
        error={form.errors.grantTypes}
        onChange={(value) => {
          form.setFieldValue("grantTypes", value as OAuthGrantType[]);
        }}
      >
        <Stack mt="xs" gap="xs">
          {oauthGrantTypeValues.map((grantType) => (
            <Checkbox
              key={grantType}
              disabled={disabled}
              value={grantType}
              label={t(oauthGrantTypeTranslationKeys[grantType])}
            />
          ))}
        </Stack>
      </Checkbox.Group>
      <Checkbox.Group
        label={t("responseTypes")}
        value={form.values.responseTypes}
        error={form.errors.responseTypes}
        onChange={(value) => {
          form.setFieldValue("responseTypes", value as OAuthResponseType[]);
        }}
      >
        <Stack mt="xs" gap="xs">
          {oauthResponseTypeValues.map((responseType) => (
            <Checkbox
              key={responseType}
              disabled={disabled}
              value={responseType}
              label={t(oauthResponseTypeTranslationKeys[responseType])}
            />
          ))}
        </Stack>
      </Checkbox.Group>
      <TagsInput
        disabled={disabled}
        variant="filled"
        label={t("redirectUris")}
        {...form.getInputProps("redirectUris")}
      />
      <TagsInput
        disabled={disabled}
        variant="filled"
        label={t("postLogoutRedirectUris")}
        description={t("postLogoutRedirectUrisDescription")}
        {...form.getInputProps("postLogoutRedirectUris")}
      />
      <TextInput
        readOnly={disabled}
        variant="filled"
        label={t("backchannelLogoutUri")}
        description={t("backchannelLogoutUriDescription")}
        {...form.getInputProps("backchannelLogoutUri")}
      />
      <Checkbox
        disabled={disabled}
        label={t("backchannelLogoutSessionRequired")}
        description={t("backchannelLogoutSessionRequiredDescription")}
        {...form.getInputProps("backchannelLogoutSessionRequired", {
          type: "checkbox",
        })}
      />
      <TagsInput
        disabled={disabled}
        variant="filled"
        label={t("scopes")}
        {...form.getInputProps("scopes")}
      />
      <TagsInput
        disabled={disabled}
        variant="filled"
        label={t("audiences")}
        {...form.getInputProps("audiences")}
      />
    </Stack>
  );
};

export default OAuthClientFormFields;
