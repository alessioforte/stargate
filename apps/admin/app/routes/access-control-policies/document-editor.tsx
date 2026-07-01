import {
  Alert,
  Badge,
  Box,
  Group,
  Paper,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { CodeBox } from "@/components";
import { useTranslations } from "@/i18n";
import type { PolicyDiagnostic } from "@/services/types";

interface Props {
  content: string;
  diagnostics: PolicyDiagnostic[];
  dirty: boolean;
  valid?: boolean;
  onChange: (value: string) => void;
}

const DocumentEditor: React.FC<Props> = ({
  content,
  diagnostics,
  dirty,
  valid,
  onChange,
}) => {
  const t = useTranslations();

  return (
    <Stack gap="sm">
      {/*<Group justify="space-between" align="center">
          <Group gap="xs">
            <Title order={5}>{t("policyDocument")}</Title>
            {dirty && (
              <Badge color="yellow" variant="light">
                {t("unsavedChanges")}
              </Badge>
            )}
            {valid !== undefined && (
              <Badge color={valid ? "teal" : "red"} variant="light">
                {valid ? t("valid") : t("invalid")}
              </Badge>
            )}
          </Group>
        </Group>*/}

      {/*{dirty && valid !== true && (
        <Alert color="yellow" variant="light">
          {t("validateBeforeSaving")}
        </Alert>
      )}*/}

      {diagnostics.length > 0 && (
        <Alert color="red" variant="light">
          <Stack gap={4}>
            {diagnostics.slice(0, 5).map((diagnostic) => (
              <Text key={`${diagnostic.line}-${diagnostic.message}`} size="sm">
                {t("line")} {diagnostic.line}: {diagnostic.message}
              </Text>
            ))}
          </Stack>
        </Alert>
      )}

      <Box>
        <CodeBox language="yaml" value={content} onChange={onChange} />
      </Box>
    </Stack>
  );
};

export default DocumentEditor;
