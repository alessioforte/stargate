import { Badge, Button, Group, Title } from "@mantine/core";
import { FiRefreshCw, FiRotateCcw, FiSave } from "react-icons/fi";
import { LuWandSparkles } from "react-icons/lu";
import { useTranslations } from "@/i18n";

interface Props {
  dirty: boolean;
  loading: boolean;
  revision?: string;
  saveDisabled: boolean;
  valid?: boolean;
  validating: boolean;
  onReload: () => void;
  onReset: () => void;
  onSave: () => void;
  onValidate: () => void;
}

const AccessControlToolbar: React.FC<Props> = ({
  dirty,
  loading,
  revision,
  saveDisabled,
  valid,
  validating,
  onReload,
  onReset,
  onSave,
  onValidate,
}) => {
  const t = useTranslations();

  return (
    <Group p="xs" justify="space-between">
      <Group gap="xs">
        <Title order={4}>{t("accessControlPolicies")}</Title>
        {revision && <Badge color="cyan">{revision.slice(0, 19)}</Badge>}
        {valid !== undefined && (
          <Badge color={valid ? "teal" : "red"}>
            {valid ? t("valid") : t("invalid")}
          </Badge>
        )}
        {dirty && <Badge color="orange">{t("unsavedChanges")}</Badge>}
      </Group>

      <Group gap="xs">
        <Button
          type="button"
          size="compact-sm"
          color="gray"
          variant="transparent"
          leftSection={<FiRefreshCw />}
          loading={loading}
          onClick={onReload}
        >
          {t("reload")}
        </Button>
        <Button
          type="button"
          size="compact-sm"
          color="teal"
          variant="outline"
          leftSection={<FiRotateCcw />}
          disabled={!dirty}
          onClick={onReset}
        >
          {t("resetChanges")}
        </Button>
        <Button
          type="button"
          size="compact-sm"
          color="indigo"
          leftSection={<LuWandSparkles />}
          loading={validating}
          onClick={onValidate}
        >
          {t("validateDraft")}
        </Button>
        <Button
          type="button"
          size="compact-sm"
          color="teal"
          leftSection={<FiSave />}
          disabled={saveDisabled}
          loading={loading}
          onClick={onSave}
        >
          {t("saveChanges")}
        </Button>
      </Group>
    </Group>
  );
};

export default AccessControlToolbar;
