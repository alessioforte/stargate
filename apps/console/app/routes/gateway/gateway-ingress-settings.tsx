import { Group, Select, TextInput } from "@mantine/core";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import { getPathRecord, isRecord, type JsonRecord } from "./config-utils";

interface Props {
  config: JsonValue | null;
  onChange: (ingress: JsonRecord) => void;
}

const GatewayIngressSettings: React.FC<Props> = ({ config, onChange }) => {
  const t = useTranslations();
  const ingress = getPathRecord(config, ["ingress"]);
  const limits = getPathRecord(config, ["limits"]);
  const limitNames = Object.entries(limits)
    .filter(
      ([, limit]) =>
        isRecord(limit) &&
        ["gcra", "token_bucket"].includes(String(limit.strategy)),
    )
    .map(([name]) => name)
    .sort();

  return (
    <Group align="start" grow>
      <Select
        label={t("ingressLimit")}
        description={t("ingressLimitDescription")}
        data={limitNames}
        value={typeof ingress.limit === "string" ? ingress.limit : null}
        allowDeselect={false}
        required
        onChange={(limit) => limit && onChange({ ...ingress, limit })}
      />
      <TextInput
        label={t("ingressTimeout")}
        description={t("ingressTimeoutDescription")}
        value={typeof ingress.timeout === "string" ? ingress.timeout : "250ms"}
        required
        onChange={(event) =>
          onChange({ ...ingress, timeout: event.currentTarget.value })
        }
      />
    </Group>
  );
};

export default GatewayIngressSettings;
