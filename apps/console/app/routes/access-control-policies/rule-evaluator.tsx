import {
  Alert,
  Badge,
  Box,
  Button,
  Group,
  Select,
  SimpleGrid,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useForm } from "@mantine/form";
import { GoShieldCheck, GoShieldX } from "react-icons/go";
import { LuTestTube } from "react-icons/lu";
import { useTranslations } from "@/i18n";
import type {
  AccessControlResourceAction,
  EvaluateAccessControlRequest,
  EvaluateAccessControlResponse,
  JsonValue,
} from "@/services/types";
import { CodeBox } from "@/components";

interface FormValues {
  action: AccessControlResourceAction | "";
  context: string;
  resource: string;
  subject: string;
}

interface Props {
  error?: string;
  loading: boolean;
  result?: EvaluateAccessControlResponse | null;
  onEvaluate: (
    request: EvaluateAccessControlRequest,
  ) => Promise<EvaluateAccessControlResponse | null>;
}

const DEFAULT_CONTEXT = `{
  "user.role": "admin"
}`;

const actionOptions: { value: AccessControlResourceAction; label: string }[] = [
  "READ",
  "WRITE",
  "DELETE",
  "CREATE",
  "UPDATE",
  "EXECUTE",
  "ADMIN",
  "ANY",
  "*",
].map((value) => ({
  value: value as AccessControlResourceAction,
  label: value,
}));

function parseContext(value: string): Record<string, JsonValue> {
  const parsed = JSON.parse(value) as JsonValue;
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("invalidContextJson");
  }

  return parsed as Record<string, JsonValue>;
}

const RuleEvaluator: React.FC<Props> = ({
  error,
  loading,
  result,
  onEvaluate,
}) => {
  const t = useTranslations();
  const form = useForm<FormValues>({
    initialValues: {
      action: "READ",
      context: DEFAULT_CONTEXT,
      resource: "reports",
      subject: "user",
    },
    validate: {
      context: (value) => {
        try {
          parseContext(value);
          return null;
        } catch {
          return t("invalidContextJson");
        }
      },
      resource: (value) =>
        value.trim().length > 0 ? null : t("resourceRequired"),
      subject: (value) =>
        value.trim().length > 0 ? null : t("subjectRequired"),
    },
  });

  const handleSubmit = (values: FormValues) => {
    return onEvaluate({
      action: values.action || null,
      context: parseContext(values.context),
      resource: values.resource.trim(),
      subject: values.subject.trim(),
    });
  };

  return (
    <form onSubmit={form.onSubmit(handleSubmit)}>
      <Stack gap="sm">
        <Box p="xs">
          <Group justify="space-between">
            <Title order={5}>{t("ruleEvaluator")}</Title>
            <Button
              type="submit"
              leftSection={<LuTestTube />}
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
          <Group justify="space-between" h={60}>
            <Group style={{ flex: 1 }} grow>
              {result && (
                <SimpleGrid cols={{ base: 1, sm: 4 }} spacing="sm">
                  <Box>
                    <Text size="xs" c="dimmed">
                      {t("allowCount")}
                    </Text>
                    <Text fw={700}>{result.allowCount}</Text>
                  </Box>
                  <Box>
                    <Text size="xs" c="dimmed">
                      {t("denyCount")}
                    </Text>
                    <Text fw={700}>{result.denyCount}</Text>
                  </Box>
                  <Box>
                    <Text size="xs" c="dimmed">
                      {t("matchedPolicies")}
                    </Text>
                    <Text fw={700}>
                      {result.matchedPolicies.join(", ") || "-"}
                    </Text>
                  </Box>
                  <Box>
                    <Text size="xs" c="dimmed">
                      {t("appliedPolicies")}
                    </Text>
                    <Text fw={700}>
                      {result.appliedPolicies.join(", ") || "-"}
                    </Text>
                  </Box>
                </SimpleGrid>
              )}
            </Group>
            {result && (
              <Badge
                color={result.allowed ? "teal" : "red"}
                leftSection={result.allowed ? <GoShieldCheck /> : <GoShieldX />}
              >
                {result.allowed ? t("allowed") : t("denied")}
              </Badge>
            )}
          </Group>

          <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="xs">
            <TextInput
              withAsterisk
              label={t("subject")}
              {...form.getInputProps("subject")}
            />
            <TextInput
              withAsterisk
              label={t("resource")}
              {...form.getInputProps("resource")}
            />
            <Select
              clearable
              label={t("action")}
              data={actionOptions}
              value={form.values.action || null}
              onChange={(value) =>
                form.setFieldValue(
                  "action",
                  (value as AccessControlResourceAction | null) ?? "",
                )
              }
            />
          </SimpleGrid>
        </Box>
        <Box px="xs">
          <Text size="sm" fw={700}>
            {t("contextJson")}
          </Text>
        </Box>
        <CodeBox
          value={form.values.context}
          onChange={(value) => form.setFieldValue("context", value)}
        />
      </Stack>
    </form>
  );
};

export default RuleEvaluator;
