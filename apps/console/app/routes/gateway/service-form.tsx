import { useEffect, useMemo, useState } from "react";
import {
  ActionIcon,
  Box,
  Divider,
  Group,
  NumberInput,
  Select,
  Stack,
  Text,
  Textarea,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { FaPlus } from "react-icons/fa6";
import { IoCloseSharp } from "react-icons/io5";
import { CodeBox } from "@/components";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import { isRecord, stringifyJson, type JsonRecord } from "./config-utils";
import GatewayEntityForm from "./gateway-entity-form";
import {
  getObjectNameErrorKey,
  nameOptions,
  positiveInteger,
  type GatewayNamedFormProps,
} from "./gateway-form-utils";
import HeaderValueRows, { type HeaderValueRow } from "./header-value-rows";
import {
  isServiceKind,
  responseBodyKindOptions,
  serviceKindOptions,
  type ResponseBodyKind,
  type ServiceKind,
} from "./schema-options";

interface Props extends GatewayNamedFormProps {
  serviceNames: string[];
  upstreamNames: string[];
}

interface WeightedServiceValues {
  id: string;
  name: string;
  weight: number | string;
}

interface MirrorServiceValues {
  id: string;
  service: string;
  percent: number | string;
}

interface FailoverValues {
  id: string;
  service: string;
}

interface StatusValues {
  id: string;
  status: number | string;
}

interface FormValues {
  name: string;
  kind: ServiceKind;
  upstream: string;
  weightedServices: WeightedServiceValues[];
  mirrorService: string;
  mirrors: MirrorServiceValues[];
  failoverService: string;
  failovers: FailoverValues[];
  onStatus: StatusValues[];
  directStatus: number | string;
  headers: HeaderValueRow[];
  bodyKind: ResponseBodyKind;
  bodyText: string;
  bodyJson: string;
}

interface BuildContext {
  serviceNames: string[];
  upstreamNames: string[];
}

interface BuildResult {
  errorKey: string | null;
  value: JsonValue | null;
}

const weightedMax = 65_535;
let rowId = 0;

function nextRowId(prefix: string) {
  rowId += 1;
  return `${prefix}-${rowId}`;
}

function createWeightedService(
  name = "",
  weight: number | string = 100,
): WeightedServiceValues {
  return {
    id: nextRowId("weighted-service"),
    name,
    weight,
  };
}

function createMirrorService(
  service = "",
  percent: number | string = 100,
): MirrorServiceValues {
  return {
    id: nextRowId("mirror-service"),
    service,
    percent,
  };
}

function createFailover(service = ""): FailoverValues {
  return {
    id: nextRowId("failover"),
    service,
  };
}

function createStatus(status: number | string = 502): StatusValues {
  return {
    id: nextRowId("status"),
    status,
  };
}

function createHeader(name = "", value = ""): HeaderValueRow {
  return {
    id: nextRowId("header"),
    name,
    value,
  };
}

function defaultFormValues(selectedName: string): FormValues {
  return {
    name: selectedName,
    kind: "load_balancer",
    upstream: "",
    weightedServices: [createWeightedService()],
    mirrorService: "",
    mirrors: [createMirrorService()],
    failoverService: "",
    failovers: [createFailover()],
    onStatus: [],
    directStatus: 200,
    headers: [],
    bodyKind: "none",
    bodyText: "",
    bodyJson: "{}",
  };
}

function stringValue(value: JsonValue | undefined) {
  return typeof value === "string" ? value : "";
}

function numberValue(value: JsonValue | undefined, fallback: number | string) {
  return typeof value === "number" ? value : fallback;
}

function weightedServicesToForm(value: JsonValue): WeightedServiceValues[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) => {
    if (!isRecord(item)) return [];
    return [
      createWeightedService(
        stringValue(item.name),
        numberValue(item.weight, 100),
      ),
    ];
  });
}

function mirrorsToForm(value: JsonValue): MirrorServiceValues[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) => {
    if (!isRecord(item)) return [];
    return [
      createMirrorService(
        stringValue(item.service),
        numberValue(item.percent, 100),
      ),
    ];
  });
}

function failoversToForm(value: JsonValue): FailoverValues[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) =>
    typeof item === "string" ? [createFailover(item)] : [],
  );
}

function statusesToForm(value: JsonValue): StatusValues[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) =>
    typeof item === "number" ? [createStatus(item)] : [],
  );
}

function headersToForm(value: JsonValue): HeaderValueRow[] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) => {
    if (!isRecord(item)) return [];
    return [createHeader(stringValue(item.name), stringValue(item.value))];
  });
}

function responseBodyToForm(value: JsonValue | undefined) {
  if (!isRecord(value)) {
    return {
      bodyKind: "none" as ResponseBodyKind,
      bodyText: "",
      bodyJson: "{}",
    };
  }

  if (typeof value.text === "string") {
    return {
      bodyKind: "text" as ResponseBodyKind,
      bodyText: value.text,
      bodyJson: "{}",
    };
  }

  if ("json" in value) {
    return {
      bodyKind: "json" as ResponseBodyKind,
      bodyText: "",
      bodyJson: stringifyJson(value.json),
    };
  }

  return {
    bodyKind: "none" as ResponseBodyKind,
    bodyText: "",
    bodyJson: "{}",
  };
}

function serviceToFormValues(
  selectedName: string,
  selectedValue: JsonValue | null,
): FormValues {
  const defaults = defaultFormValues(selectedName);
  if (!isRecord(selectedValue)) return defaults;

  const kind = isServiceKind(selectedValue.kind)
    ? selectedValue.kind
    : defaults.kind;
  const body = responseBodyToForm(selectedValue.body);
  const weightedServices = weightedServicesToForm(selectedValue.services);
  const mirrors = mirrorsToForm(selectedValue.mirrors);
  const failovers = failoversToForm(selectedValue.failovers);

  return {
    ...defaults,
    ...body,
    kind,
    upstream: stringValue(selectedValue.upstream),
    weightedServices: weightedServices.length
      ? weightedServices
      : defaults.weightedServices,
    mirrorService: stringValue(selectedValue.service),
    mirrors: mirrors.length ? mirrors : defaults.mirrors,
    failoverService: stringValue(selectedValue.service),
    failovers: failovers.length ? failovers : defaults.failovers,
    onStatus: statusesToForm(selectedValue.on_status),
    directStatus: numberValue(selectedValue.status, defaults.directStatus),
    headers: headersToForm(selectedValue.headers),
  };
}

function hasDuplicate(values: string[]) {
  const seen = new Set<string>();
  for (const value of values) {
    if (seen.has(value)) return true;
    seen.add(value);
  }

  return false;
}

function validateServiceReference(value: string, serviceNames: string[]) {
  if (!value) return "serviceReferenceRequired";
  if (!serviceNames.includes(value)) return "unknownServiceReference";
  return null;
}

function validHttpStatus(value: number | string) {
  const status = positiveInteger(value, 599);
  return status && status >= 100 ? status : null;
}

function formValuesToService(
  values: FormValues,
  context: BuildContext,
): BuildResult {
  switch (values.kind) {
    case "load_balancer": {
      if (!values.upstream)
        return { errorKey: "upstreamRequired", value: null };
      if (!context.upstreamNames.includes(values.upstream)) {
        return { errorKey: "unknownUpstreamReference", value: null };
      }

      return {
        errorKey: null,
        value: {
          kind: "load_balancer",
          upstream: values.upstream,
        },
      };
    }
    case "weighted": {
      if (values.weightedServices.length === 0) {
        return { errorKey: "weightedServicesRequired", value: null };
      }

      const services: JsonRecord[] = [];
      for (const row of values.weightedServices) {
        const name = row.name.trim();
        const referenceError = validateServiceReference(
          name,
          context.serviceNames,
        );
        if (referenceError) return { errorKey: referenceError, value: null };

        const weight = positiveInteger(row.weight, weightedMax);
        if (!weight) return { errorKey: "invalidServiceWeight", value: null };
        services.push({ name, weight });
      }

      if (hasDuplicate(services.map((service) => String(service.name)))) {
        return { errorKey: "duplicateServiceReference", value: null };
      }

      return {
        errorKey: null,
        value: {
          kind: "weighted",
          services,
        },
      };
    }
    case "mirror": {
      const primary = values.mirrorService.trim();
      const primaryError = validateServiceReference(
        primary,
        context.serviceNames,
      );
      if (primaryError) return { errorKey: primaryError, value: null };
      if (values.mirrors.length === 0) {
        return { errorKey: "mirrorsRequired", value: null };
      }

      const mirrors: JsonRecord[] = [];
      for (const row of values.mirrors) {
        const service = row.service.trim();
        const referenceError = validateServiceReference(
          service,
          context.serviceNames,
        );
        if (referenceError) return { errorKey: referenceError, value: null };

        const percent = positiveInteger(row.percent, 100);
        if (!percent) return { errorKey: "invalidMirrorPercent", value: null };
        mirrors.push({ service, percent });
      }

      if (hasDuplicate(mirrors.map((mirror) => String(mirror.service)))) {
        return { errorKey: "duplicateServiceReference", value: null };
      }

      return {
        errorKey: null,
        value: {
          kind: "mirror",
          service: primary,
          mirrors,
        },
      };
    }
    case "failover": {
      const primary = values.failoverService.trim();
      const primaryError = validateServiceReference(
        primary,
        context.serviceNames,
      );
      if (primaryError) return { errorKey: primaryError, value: null };
      if (values.failovers.length === 0) {
        return { errorKey: "failoversRequired", value: null };
      }

      const failovers: string[] = [];
      for (const row of values.failovers) {
        const service = row.service.trim();
        const referenceError = validateServiceReference(
          service,
          context.serviceNames,
        );
        if (referenceError) return { errorKey: referenceError, value: null };
        failovers.push(service);
      }

      if (hasDuplicate(failovers)) {
        return { errorKey: "duplicateServiceReference", value: null };
      }

      const onStatus: number[] = [];
      for (const row of values.onStatus) {
        const status = validHttpStatus(row.status);
        if (!status) return { errorKey: "invalidHttpStatus", value: null };
        onStatus.push(status);
      }

      if (hasDuplicate(onStatus.map(String))) {
        return { errorKey: "duplicateStatusCode", value: null };
      }

      return {
        errorKey: null,
        value: {
          kind: "failover",
          service: primary,
          failovers,
          on_status: onStatus,
        },
      };
    }
    case "direct_response": {
      const status = validHttpStatus(values.directStatus);
      if (!status) return { errorKey: "invalidHttpStatus", value: null };

      const headers: JsonRecord[] = [];
      for (const row of values.headers) {
        const name = row.name.trim();
        if (!name && !row.value.trim()) continue;
        if (!name) return { errorKey: "headerNameRequired", value: null };
        headers.push({ name, value: row.value });
      }

      const service: JsonRecord = {
        kind: "direct_response",
        status,
        headers,
      };

      if (values.bodyKind === "text") {
        service.body = { text: values.bodyText };
      }

      if (values.bodyKind === "json") {
        try {
          service.body = { json: JSON.parse(values.bodyJson) as JsonValue };
        } catch {
          return { errorKey: "invalidJson", value: null };
        }
      }

      return { errorKey: null, value: service };
    }
  }
}

const ServiceForm: React.FC<Props> = ({
  canDelete,
  existingNames,
  selectedName,
  selectedValue,
  serviceNames,
  upstreamNames,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const [values, setValues] = useState<FormValues>(() =>
    serviceToFormValues(selectedName, selectedValue),
  );
  const [error, setError] = useState<string | null>(null);
  const upstreamOptions = useMemo(
    () => nameOptions(upstreamNames),
    [upstreamNames],
  );
  const serviceOptions = useMemo(
    () => nameOptions(serviceNames.filter((name) => name !== selectedName)),
    [selectedName, serviceNames],
  );
  const availableServiceNames = useMemo(
    () => serviceNames.filter((name) => name !== selectedName),
    [selectedName, serviceNames],
  );

  useEffect(() => {
    setValues(serviceToFormValues(selectedName, selectedValue));
    setError(null);
  }, [selectedName, selectedValue]);

  const setField = <Key extends keyof FormValues>(
    key: Key,
    value: FormValues[Key],
  ) => {
    setValues((current) => ({ ...current, [key]: value }));
  };

  const updateWeightedService = (
    id: string,
    patch: Partial<WeightedServiceValues>,
  ) => {
    setValues((current) => ({
      ...current,
      weightedServices: current.weightedServices.map((service) =>
        service.id === id ? { ...service, ...patch } : service,
      ),
    }));
  };

  const removeWeightedService = (id: string) => {
    setValues((current) => ({
      ...current,
      weightedServices: current.weightedServices.filter(
        (service) => service.id !== id,
      ),
    }));
  };

  const updateMirror = (id: string, patch: Partial<MirrorServiceValues>) => {
    setValues((current) => ({
      ...current,
      mirrors: current.mirrors.map((mirror) =>
        mirror.id === id ? { ...mirror, ...patch } : mirror,
      ),
    }));
  };

  const removeMirror = (id: string) => {
    setValues((current) => ({
      ...current,
      mirrors: current.mirrors.filter((mirror) => mirror.id !== id),
    }));
  };

  const updateFailover = (id: string, patch: Partial<FailoverValues>) => {
    setValues((current) => ({
      ...current,
      failovers: current.failovers.map((failover) =>
        failover.id === id ? { ...failover, ...patch } : failover,
      ),
    }));
  };

  const removeFailover = (id: string) => {
    setValues((current) => ({
      ...current,
      failovers: current.failovers.filter((failover) => failover.id !== id),
    }));
  };

  const updateStatus = (id: string, patch: Partial<StatusValues>) => {
    setValues((current) => ({
      ...current,
      onStatus: current.onStatus.map((status) =>
        status.id === id ? { ...status, ...patch } : status,
      ),
    }));
  };

  const removeStatus = (id: string) => {
    setValues((current) => ({
      ...current,
      onStatus: current.onStatus.filter((status) => status.id !== id),
    }));
  };

  const updateHeader = (id: string, patch: Partial<HeaderValueRow>) => {
    setValues((current) => ({
      ...current,
      headers: current.headers.map((header) =>
        header.id === id ? { ...header, ...patch } : header,
      ),
    }));
  };

  const removeHeader = (id: string) => {
    setValues((current) => ({
      ...current,
      headers: current.headers.filter((header) => header.id !== id),
    }));
  };

  const handleApply = () => {
    const trimmedName = values.name.trim();
    const nameErrorKey = getObjectNameErrorKey(
      trimmedName,
      selectedName,
      existingNames,
    );
    if (nameErrorKey) {
      setError(t(nameErrorKey));
      return;
    }

    const result = formValuesToService(values, {
      serviceNames: availableServiceNames,
      upstreamNames,
    });
    if (!result.value) {
      setError(t(result.errorKey ?? "invalidServiceConfig"));
      return;
    }

    setError(null);
    onApply(selectedName, trimmedName, result.value);
  };

  return (
    <GatewayEntityForm
      canDelete={canDelete}
      error={error}
      selectedName={selectedName}
      onApply={handleApply}
      onDelete={onDelete}
    >
      <Stack p="xs" gap="xs">
        <TextInput
          variant="filled"
          label={t("name")}
          value={values.name}
          onChange={(event) => setField("name", event.currentTarget.value)}
        />
        <Select
          allowDeselect={false}
          variant="filled"
          label={t("kind")}
          data={serviceKindOptions}
          value={values.kind}
          onChange={(value) =>
            setField("kind", (value ?? values.kind) as ServiceKind)
          }
        />
      </Stack>

      <Divider size={3} />

      {values.kind === "load_balancer" && (
        <Stack p="xs" gap="xs">
          <Select
            searchable
            variant="filled"
            label={t("upstream")}
            placeholder={t("selectUpstream")}
            data={upstreamOptions}
            value={values.upstream || null}
            onChange={(value) => setField("upstream", value ?? "")}
          />
        </Stack>
      )}

      {values.kind === "weighted" && (
        <Stack p="xs" gap="xs">
          <Group justify="space-between">
            <Text fw={700}>{t("weightedServices")}</Text>
            <Tooltip label={t("addService")} position="left" offset={10}>
              <ActionIcon
                size="sm"
                onClick={() =>
                  setField("weightedServices", [
                    ...values.weightedServices,
                    createWeightedService(),
                  ])
                }
              >
                <FaPlus />
              </ActionIcon>
            </Tooltip>
          </Group>
          {values.weightedServices.map((service) => (
            <Box
              key={service.id}
              p="xs"
              style={{
                border: "1px solid var(--mantine-color-default-border)",
                borderRadius: 6,
              }}
            >
              <Group align="center" wrap="nowrap">
                <Group
                  justify="space-between"
                  wrap="nowrap"
                  style={{ flex: 1 }}
                >
                  <Select
                    size="xs"
                    searchable
                    variant="filled"
                    style={{ flex: 1 }}
                    label={t("service")}
                    data={serviceOptions}
                    value={service.name || null}
                    onChange={(name) =>
                      updateWeightedService(service.id, { name: name ?? "" })
                    }
                  />
                  <NumberInput
                    size="xs"
                    step={1}
                    min={1}
                    max={weightedMax}
                    style={{ maxWidth: 80 }}
                    allowDecimal={false}
                    variant="filled"
                    label={t("weight")}
                    value={service.weight}
                    onChange={(weight) =>
                      updateWeightedService(service.id, { weight })
                    }
                  />
                </Group>
                <Tooltip label={t("delete")} position="left">
                  <ActionIcon
                    type="button"
                    mt={25}
                    color="red"
                    variant="transparent"
                    disabled={values.weightedServices.length === 1}
                    onClick={() => removeWeightedService(service.id)}
                  >
                    <IoCloseSharp />
                  </ActionIcon>
                </Tooltip>
              </Group>
            </Box>
          ))}
        </Stack>
      )}

      {values.kind === "mirror" && (
        <Stack p="xs" gap="xs">
          <Select
            searchable
            variant="filled"
            label={t("primaryService")}
            data={serviceOptions}
            value={values.mirrorService || null}
            onChange={(service) => setField("mirrorService", service ?? "")}
          />
          <Group mt="md" justify="space-between">
            <Text fw={700}>{t("mirrorServices")}</Text>

            <Tooltip label={t("addMirror")} position="left" offset={10}>
              <ActionIcon
                size="sm"
                onClick={() =>
                  setField("mirrors", [
                    ...values.mirrors,
                    createMirrorService(),
                  ])
                }
              >
                <FaPlus />
              </ActionIcon>
            </Tooltip>
          </Group>
          {values.mirrors.map((mirror) => (
            <Box
              key={mirror.id}
              p="xs"
              style={{
                border: "1px solid var(--mantine-color-default-border)",
                borderRadius: 6,
              }}
            >
              <Group align="center" wrap="nowrap">
                <Group
                  justify="space-between"
                  wrap="nowrap"
                  style={{ flex: 1 }}
                >
                  <Select
                    size="xs"
                    searchable
                    style={{ flex: 1 }}
                    variant="filled"
                    label={t("service")}
                    data={serviceOptions}
                    value={mirror.service || null}
                    onChange={(service) =>
                      updateMirror(mirror.id, { service: service ?? "" })
                    }
                  />
                  <NumberInput
                    min={1}
                    max={100}
                    step={1}
                    size="xs"
                    style={{ maxWidth: 80 }}
                    allowDecimal={false}
                    variant="filled"
                    label={t("percent")}
                    value={mirror.percent}
                    onChange={(percent) => updateMirror(mirror.id, { percent })}
                  />
                </Group>
                <Tooltip label={t("delete")}>
                  <ActionIcon
                    type="button"
                    mt={25}
                    color="red"
                    variant="transparent"
                    disabled={values.mirrors.length === 1}
                    onClick={() => removeMirror(mirror.id)}
                  >
                    <IoCloseSharp />
                  </ActionIcon>
                </Tooltip>
              </Group>
            </Box>
          ))}
        </Stack>
      )}

      {values.kind === "failover" && (
        <>
          <Stack p="xs" gap="xs">
            <Text size="sm" c="dimmed">
              {t("failoverReplayDescription")}
            </Text>
            <Select
              searchable
              variant="filled"
              label={t("primaryService")}
              data={serviceOptions}
              value={values.failoverService || null}
              onChange={(service) => setField("failoverService", service ?? "")}
            />
            <Group mt="md" justify="space-between">
              <Text fw={700}>{t("failoverServices")}</Text>

              <Tooltip label={t("addFailover")} position="left" offset={10}>
                <ActionIcon
                  size="sm"
                  onClick={() =>
                    setField("failovers", [
                      ...values.failovers,
                      createFailover(),
                    ])
                  }
                >
                  <FaPlus />
                </ActionIcon>
              </Tooltip>
            </Group>
            {values.failovers.map((failover) => (
              <Group key={failover.id} align="center" wrap="nowrap">
                <Select
                  searchable
                  size="xs"
                  variant="filled"
                  label={t("service")}
                  data={serviceOptions}
                  value={failover.service || null}
                  onChange={(service) =>
                    updateFailover(failover.id, { service: service ?? "" })
                  }
                  style={{ flex: 1 }}
                />
                <Tooltip label={t("delete")}>
                  <ActionIcon
                    type="button"
                    mt={25}
                    color="red"
                    variant="transparent"
                    disabled={values.failovers.length === 1}
                    onClick={() => removeFailover(failover.id)}
                  >
                    <IoCloseSharp />
                  </ActionIcon>
                </Tooltip>
              </Group>
            ))}
          </Stack>

          <Divider size={3} />

          <Stack p="xs" gap="xs">
            <Group justify="space-between">
              <Text fw={700}>{t("failoverStatusCodes")}</Text>

              <Tooltip label={t("addStatusCode")} position="left" offset={10}>
                <ActionIcon
                  size="sm"
                  onClick={() =>
                    setField("onStatus", [...values.onStatus, createStatus()])
                  }
                >
                  <FaPlus />
                </ActionIcon>
              </Tooltip>
            </Group>
            {values.onStatus.length === 0 && (
              <Text size="sm" c="dimmed">
                {t("allTransportErrors")}
              </Text>
            )}
            {values.onStatus.map((status) => (
              <Group key={status.id} align="center" wrap="nowrap">
                <NumberInput
                  min={100}
                  max={599}
                  step={1}
                  size="xs"
                  allowDecimal={false}
                  variant="filled"
                  label={t("statusCode")}
                  value={status.status}
                  onChange={(value) =>
                    updateStatus(status.id, { status: value })
                  }
                  style={{ flex: 1 }}
                />
                <Tooltip label={t("delete")}>
                  <ActionIcon
                    type="button"
                    mt={25}
                    color="red"
                    variant="transparent"
                    onClick={() => removeStatus(status.id)}
                  >
                    <IoCloseSharp />
                  </ActionIcon>
                </Tooltip>
              </Group>
            ))}
          </Stack>
        </>
      )}

      {values.kind === "direct_response" && (
        <>
          <Stack p="xs" gap="xs">
            <NumberInput
              min={100}
              max={599}
              step={1}
              allowDecimal={false}
              variant="filled"
              label={t("statusCode")}
              value={values.directStatus}
              onChange={(status) => setField("directStatus", status)}
            />

            <HeaderValueRows
              rows={values.headers}
              titleKey="headers"
              onAdd={() =>
                setField("headers", [...values.headers, createHeader()])
              }
              onRemove={removeHeader}
              onUpdate={updateHeader}
            />
          </Stack>

          <Divider size={3} />

          <Select
            p="xs"
            allowDeselect={false}
            variant="filled"
            label={t("responseBody")}
            data={responseBodyKindOptions}
            value={values.bodyKind}
            onChange={(kind) =>
              setField(
                "bodyKind",
                (kind ?? values.bodyKind) as ResponseBodyKind,
              )
            }
          />
          {values.bodyKind === "text" && (
            <Textarea
              radius={0}
              autosize
              minRows={3}
              variant="filled"
              value={values.bodyText}
              onChange={(event) =>
                setField("bodyText", event.currentTarget.value)
              }
            />
          )}
          {values.bodyKind === "json" && (
            <CodeBox
              value={values.bodyJson}
              onChange={(value) => setField("bodyJson", value)}
            />
          )}
        </>
      )}
    </GatewayEntityForm>
  );
};

export default ServiceForm;
