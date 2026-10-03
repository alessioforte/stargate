import { Badge, Button, Group, Title } from "@mantine/core";
import { FiRefreshCw, FiRotateCcw, FiSave } from "react-icons/fi";
import { useTranslations } from "@/i18n";
import { GATEWAY_SCHEMA } from "./config-utils";

interface Props {
  dirty: boolean;
  loading: boolean;
  saveDisabled: boolean;
  schema: string;
  onReload: () => void;
  onReset: () => void;
  onSave: () => void;
}

const GatewayToolbar: React.FC<Props> = ({
  dirty,
  loading,
  saveDisabled,
  schema,
  onReload,
  onReset,
  onSave,
}) => {
  const t = useTranslations();

  return (
    <Group p="xs" justify="space-between">
      <Group gap="xs">
        <Title order={4}>{t("gatewayConfiguration")}</Title>
        {schema && (
          <Badge color={schema === GATEWAY_SCHEMA ? "cyan" : "yellow"}>
            {schema}
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

export default GatewayToolbar;
