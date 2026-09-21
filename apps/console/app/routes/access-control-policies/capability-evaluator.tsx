import {
  Alert,
  Badge,
  Box,
  Button,
  Group,
  Select,
  SimpleGrid,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { LuListChecks } from "react-icons/lu";
import { CodeBox, Table } from "@/components";
import { useTranslations } from "@/i18n";
import type {
  AccessControlCapabilityEnvironment,
  AccessControlSubjectType,
  EvaluateAccessControlCapabilitiesRequest,
  EvaluateAccessControlCapabilitiesResponse,
  JsonValue,
} from "@/services/types";
import { accessControlCapabilityColumns } from "./capability-columns";

interface FormValues {
  attrs: string;
  env: string;
  orgId: string;
  orgRole: string;
  subject: AccessControlSubjectType;
}

interface Props {
  error?: string;
  loading: boolean;
  result?: EvaluateAccessControlCapabilitiesResponse | null;
  onEvaluate: (
    request: EvaluateAccessControlCapabilitiesRequest,
  ) => Promise<EvaluateAccessControlCapabilitiesResponse | null>;
}

const DEFAULT_ATTRIBUTES = `{
  "role": "admin"
}`;

const DEFAULT_ENVIRONMENT = "{}";

const ENVIRONMENT_KEYS = new Set([
  "cityName",
  "countryCode",
  "countryName",
  "date",
  "dayOfWeek",
  "ipAddress",
  "time",
  "userAgent",
]);

function parseObject(value: string): Record<string, JsonValue> {
  const parsed = JSON.parse(value) as JsonValue;
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("Expected a JSON object");
  }

  return parsed as Record<string, JsonValue>;
}

function parseEnvironment(value: string): AccessControlCapabilityEnvironment {
  const parsed = parseObject(value);
  for (const [key, fieldValue] of Object.entries(parsed)) {
    if (!ENVIRONMENT_KEYS.has(key) || typeof fieldValue !== "string") {
      throw new Error("Invalid environment field");
    }
  }

  return parsed as AccessControlCapabilityEnvironment;
}

function optionalValue(value: string): string | undefined {
  return value.trim() || undefined;
}

const CapabilityEvaluator: React.FC<Props> = ({
  error,
  loading,
  result,
  onEvaluate,
}) => {
  const t = useTranslations();
  const form = useForm<FormValues>({
    initialValues: {
      attrs: DEFAULT_ATTRIBUTES,
      env: DEFAULT_ENVIRONMENT,
      orgId: "",
      orgRole: "",
      subject: "user",
    },
    validate: {
      attrs: (value) => {
        try {
          parseObject(value);
          return null;
        } catch {
          return t("invalidAttributesJson");
        }
      },
      env: (value) => {
        try {
          parseEnvironment(value);
          return null;
        } catch {
          return t("invalidEnvironmentJson");
        }
      },
    },
  });

  const handleSubmit = (values: FormValues) => {
    return onEvaluate({
      attrs: parseObject(values.attrs),
      env: parseEnvironment(values.env),
      orgId: optionalValue(values.orgId),
      orgRole: optionalValue(values.orgRole),
      subject: values.subject,
    });
  };

  return (
    <form onSubmit={form.onSubmit(handleSubmit)}>
      <Group p="xs" justify="space-between">
        <Box>
          <Title order={5}>{t("capabilityEvaluator")}</Title>
          <Text size="sm" c="dimmed">
            {t("capabilityEvaluatorHint")}
          </Text>
        </Box>
        <Button
          type="submit"
          leftSection={<LuListChecks />}
          loading={loading}
          size="compact-sm"
        >
          {t("evaluate")}
        </Button>
      </Group>

      {error && (
        <Alert color="red" variant="light">
          {error}
        </Alert>
      )}

      <SimpleGrid p="xs" cols={{ base: 1, sm: 3 }} spacing="xs">
        <Select
          withAsterisk
          label={t("subject")}
          data={[
            { value: "user", label: t("user") },
            { value: "api_key", label: t("apiKey") },
          ]}
          allowDeselect={false}
          value={form.values.subject}
          onChange={(value) =>
            form.setFieldValue(
              "subject",
              (value as AccessControlSubjectType | null) ?? "user",
            )
          }
        />
        <TextInput
          label={t("organizationId")}
          placeholder={t("optional")}
          {...form.getInputProps("orgId")}
        />
        <TextInput
          label={t("organizationRole")}
          placeholder={t("optional")}
          {...form.getInputProps("orgRole")}
        />
      </SimpleGrid>

      <Group mt="xl" p="xs" justify="space-between">
        <Text size="sm" fw={700}>
          {t("attributesJson")}
        </Text>
        {form.errors.attrs && (
          <Text size="xs" c="red">
            {form.errors.attrs}
          </Text>
        )}
      </Group>
      <CodeBox
        value={form.values.attrs}
        onChange={(value) => form.setFieldValue("attrs", value)}
      />

      <Group mt="xl" p="xs" justify="space-between">
        <Box>
          <Text size="sm" fw={700}>
            {t("environmentJson")}
          </Text>
          <Text size="xs" c="dimmed">
            {t("environmentFieldsHint")}
          </Text>
        </Box>
        {form.errors.env && (
          <Text size="xs" c="red">
            {form.errors.env}
          </Text>
        )}
      </Group>
      <CodeBox
        value={form.values.env}
        onChange={(value) => form.setFieldValue("env", value)}
      />

      {result && (
        <Box mt="xl">
          <Group p="xs" gap="xs">
            <Title order={6}>{t("capabilities")}</Title>
            <Badge variant="light">{result.capabilities.length}</Badge>
            <Badge color="cyan" variant="light">
              {result.revision.slice(0, 19)}
            </Badge>
          </Group>

          {result.capabilities.length === 0 ? (
            <Alert color="gray" variant="light">
              {t("noAllowedCapabilities")}
            </Alert>
          ) : (
            <Table
              columns={accessControlCapabilityColumns}
              data={result.capabilities}
              empty={false}
              minWidth={600}
            />
          )}
        </Box>
      )}
    </form>
  );
};

export default CapabilityEvaluator;
