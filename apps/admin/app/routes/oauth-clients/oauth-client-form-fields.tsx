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
import type { OAuthGrantType, OAuthResponseType } from "@/services/types";
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
      <TextInput
        readOnly={disabled}
        variant="filled"
        label={t("orgId")}
        {...form.getInputProps("orgId")}
      />
      <TextInput
        readOnly={disabled}
        variant="filled"
        label={t("serviceAccountId")}
        {...form.getInputProps("serviceAccountId")}
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
