import { Alert, Box, Stack, Text } from "@mantine/core";
import { CodeBox } from "@/components";
import { useTranslations } from "@/i18n";
import type { PolicyDiagnostic } from "@/services/types";

interface Props {
  content: string;
  diagnostics: PolicyDiagnostic[];
  onChange: (value: string) => void;
}

const DocumentEditor: React.FC<Props> = ({
  content,
  diagnostics,
  onChange,
}) => {
  const t = useTranslations();

  return (
    <Stack gap="sm">
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
