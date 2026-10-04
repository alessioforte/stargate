import { Badge, Box, Paper, Stack, Text } from "@mantine/core";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import type { GatewaySection } from "./config-utils";
import { getPathRecord, isRecord, stringifyJson } from "./config-utils";
import LimitForm from "./limit-form";
import MiddlewareForm from "./middleware-form";
import PolicyForm from "./policy-form";
import RouterForm from "./router-form";
import ServiceForm from "./service-form";
import UpstreamForm from "./upstream-form";

interface Props {
  canDelete?: boolean;
  config: JsonValue | null;
  existingNames: string[];
  selectedName: string;
  selectedSection: GatewaySection;
  selectedValue: JsonValue | null;
  onApply: (previousName: string, nextName: string, value: JsonValue) => void;
  onDelete: (name: string) => void;
}

const GatewayObjectEditor: React.FC<Props> = ({
  canDelete = true,
  config,
  existingNames,
  selectedName,
  selectedSection,
  selectedValue,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const limits = getPathRecord(config, ["limits"]);
  const rateLimitNames = Object.keys(limits)
    .filter((name) => {
      const spec = limits[name];
      return (
        isRecord(spec) &&
        (spec.strategy === "gcra" || spec.strategy === "token_bucket")
      );
    })
    .sort();
  const quotaLimitNames = Object.keys(limits)
    .filter((name) => {
      const spec = limits[name];
      return isRecord(spec) && spec.strategy === "quota_tracker";
    })
    .sort();
  const upstreamNames = Object.keys(
    getPathRecord(config, ["http", "upstreams"]),
  ).sort();
  const middlewareNames = Object.keys(
    getPathRecord(config, ["http", "middlewares"]),
  ).sort();
  const policyNames = Object.keys(
    getPathRecord(config, ["http", "policies"]),
  ).sort();
  const serviceNames = Object.keys(
    getPathRecord(config, ["http", "services"]),
  ).sort();

  if (selectedSection.key === "limits") {
    return (
      <LimitForm
        canDelete={canDelete}
        existingNames={existingNames}
        selectedName={selectedName}
        selectedValue={selectedValue}
        onApply={onApply}
        onDelete={onDelete}
      />
    );
  }

  if (selectedSection.key === "upstreams") {
    return (
      <UpstreamForm
        canDelete={canDelete}
        existingNames={existingNames}
        selectedName={selectedName}
        selectedValue={selectedValue}
        onApply={onApply}
        onDelete={onDelete}
      />
    );
  }

  if (selectedSection.key === "services") {
    return (
      <ServiceForm
        canDelete={canDelete}
        existingNames={existingNames}
        selectedName={selectedName}
        selectedValue={selectedValue}
        serviceNames={serviceNames}
        upstreamNames={upstreamNames}
        onApply={onApply}
        onDelete={onDelete}
      />
    );
  }

  if (selectedSection.key === "middlewares") {
    return (
      <MiddlewareForm
        canDelete={canDelete}
        existingNames={existingNames}
        selectedName={selectedName}
        selectedValue={selectedValue}
        onApply={onApply}
        onDelete={onDelete}
      />
    );
  }

  if (selectedSection.key === "policies") {
    return (
      <PolicyForm
        canDelete={canDelete}
        existingNames={existingNames}
        rateLimitNames={rateLimitNames}
        quotaLimitNames={quotaLimitNames}
        selectedName={selectedName}
        selectedValue={selectedValue}
        onApply={onApply}
        onDelete={onDelete}
      />
    );
  }

  if (selectedSection.key === "routers") {
    return (
      <RouterForm
        canDelete={canDelete}
        existingNames={existingNames}
        middlewareNames={middlewareNames}
        policyNames={policyNames}
        selectedName={selectedName}
        selectedValue={selectedValue}
        serviceNames={serviceNames}
        onApply={onApply}
        onDelete={onDelete}
      />
    );
  }

  return (
    <Paper withBorder p="sm" radius="sm" style={{ flex: 1 }}>
      <Stack gap="sm">
        <Stack gap={0}>
          <Text size="xs" c="dimmed">
            {t(selectedSection.labelKey)}
          </Text>
          <Text fw={700}>{selectedName || t("selectedObject")}</Text>
        </Stack>
        <Badge color="yellow" variant="light" w="fit-content">
          {t("formComingNext")}
        </Badge>
        <Text size="sm" c="dimmed">
          {t("sectionFormComingNext")}
        </Text>
        <Box
          p="sm"
          style={{
            border: "1px solid var(--mantine-color-default-border)",
            borderRadius: 6,
            fontFamily: "monospace",
            fontSize: 13,
            whiteSpace: "pre-wrap",
          }}
        >
          {stringifyJson(selectedValue)}
        </Box>
      </Stack>
    </Paper>
  );
};

export default GatewayObjectEditor;
