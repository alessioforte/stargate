import { useEffect, useState } from "react";
import {
  ActionIcon,
  Box,
  Checkbox,
  Divider,
  Group,
  NumberInput,
  Select,
  Stack,
  Switch,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { FaPlus } from "react-icons/fa6";
import { IoCloseSharp } from "react-icons/io5";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import { isRecord, type JsonRecord } from "./config-utils";
import GatewayEntityForm from "./gateway-entity-form";
import {
  getObjectNameErrorKey,
  optionalPositiveInteger,
  type GatewayNamedFormProps,
} from "./gateway-form-utils";
import {
  isLoadBalancerStrategy,
  isUpstreamProtocol,
  loadBalancerStrategyOptions,
  upstreamProtocolOptions,
  type LoadBalancerStrategy,
  type UpstreamProtocol,
} from "./schema-options";

interface TargetValues {
  id: string;
  url: string;
  weight: number | string;
}

interface FormValues {
  name: string;
  targets: TargetValues[];
  loadBalancerStrategy: LoadBalancerStrategy;
  livenessEnabled: boolean;
  livenessPath: string;
  livenessInterval: string;
  livenessTimeout: string;
  circuitBreakerEnabled: boolean;
  failThreshold: number | string;
  cooldown: string;
  transportEnabled: boolean;
  connectTimeout: string;
  protocols: UpstreamProtocol[];
}

interface BuildResult {
  errorKey: string | null;
  value: JsonValue | null;
}

const targetWeightMax = 65_535;
let targetId = 0;

const defaultValues: FormValues = {
  name: "",
  targets: [],
  loadBalancerStrategy: "round_robin",
  livenessEnabled: false,
  livenessPath: "/health",
  livenessInterval: "",
  livenessTimeout: "",
  circuitBreakerEnabled: false,
  failThreshold: 3,
  cooldown: "30s",
  transportEnabled: false,
  connectTimeout: "",
  protocols: [],
};

function createTarget(
  url = "http://localhost:8080",
  weight: number | string = "",
) {
  targetId += 1;
  return {
    id: `target-${targetId}`,
    url,
    weight,
  };
}

function targetToFormValues(target: JsonValue): TargetValues | null {
  if (!isRecord(target)) return null;

  return createTarget(
    typeof target.url === "string" ? target.url : "",
    typeof target.weight === "number" ? target.weight : "",
  );
}

function getString(value: JsonValue | undefined, fallback = "") {
  return typeof value === "string" ? value : fallback;
}

function upstreamToFormValues(
  selectedName: string,
  selectedValue: JsonValue | null,
): FormValues {
  if (!isRecord(selectedValue)) {
    return {
      ...defaultValues,
      name: selectedName,
      targets: [createTarget()],
    };
  }

  const loadBalancer = isRecord(selectedValue.load_balancer)
    ? selectedValue.load_balancer
    : {};
  const livenessProbe = isRecord(loadBalancer.liveness_probe)
    ? loadBalancer.liveness_probe
    : null;
  const circuitBreaker = isRecord(loadBalancer.circuit_breaker)
    ? loadBalancer.circuit_breaker
    : null;
  const transport = isRecord(selectedValue.transport)
    ? selectedValue.transport
    : null;
  const targets = Array.isArray(selectedValue.targets)
    ? selectedValue.targets.flatMap((target) => {
        const targetValues = targetToFormValues(target);
        return targetValues ? [targetValues] : [];
      })
    : [];

  return {
    ...defaultValues,
    name: selectedName,
    targets: targets.length ? targets : [createTarget()],
    loadBalancerStrategy: isLoadBalancerStrategy(loadBalancer.strategy)
      ? loadBalancer.strategy
      : defaultValues.loadBalancerStrategy,
    livenessEnabled: Boolean(livenessProbe),
    livenessPath: livenessProbe
      ? getString(livenessProbe.path, defaultValues.livenessPath)
      : defaultValues.livenessPath,
    livenessInterval: livenessProbe
      ? getString(livenessProbe.interval)
      : defaultValues.livenessInterval,
    livenessTimeout: livenessProbe
      ? getString(livenessProbe.timeout)
      : defaultValues.livenessTimeout,
    circuitBreakerEnabled: Boolean(circuitBreaker),
    failThreshold:
      circuitBreaker && typeof circuitBreaker.fail_threshold === "number"
        ? circuitBreaker.fail_threshold
        : defaultValues.failThreshold,
    cooldown: circuitBreaker
      ? getString(circuitBreaker.cooldown, defaultValues.cooldown)
      : defaultValues.cooldown,
    transportEnabled: Boolean(transport),
    connectTimeout: transport
      ? getString(transport.connect_timeout)
      : defaultValues.connectTimeout,
    protocols:
      transport && Array.isArray(transport.protocols)
        ? transport.protocols.filter(isUpstreamProtocol)
        : defaultValues.protocols,
  };
}

function isHttpTargetUrl(value: string) {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function formValuesToUpstream(values: FormValues): BuildResult {
  if (values.targets.length === 0) {
    return { errorKey: "targetRequired", value: null };
  }

  const targets: JsonRecord[] = [];
  for (const target of values.targets) {
    const url = target.url.trim();
    if (!url) return { errorKey: "targetUrlRequired", value: null };
    if (!isHttpTargetUrl(url))
      return { errorKey: "invalidTargetUrl", value: null };

    const weight = optionalPositiveInteger(target.weight, targetWeightMax);
    if (weight === null) {
      return { errorKey: "invalidTargetWeight", value: null };
    }

    const nextTarget: JsonRecord = { url };
    if (weight !== undefined) nextTarget.weight = weight;
    targets.push(nextTarget);
  }

  const loadBalancer: JsonRecord = {
    strategy: values.loadBalancerStrategy,
  };

  if (values.livenessEnabled) {
    const path = values.livenessPath.trim();
    if (!path) return { errorKey: "livenessPathRequired", value: null };

    const livenessProbe: JsonRecord = { path };
    const interval = values.livenessInterval.trim();
    const timeout = values.livenessTimeout.trim();
    if (interval) livenessProbe.interval = interval;
    if (timeout) livenessProbe.timeout = timeout;
    loadBalancer.liveness_probe = livenessProbe;
  }

  if (values.circuitBreakerEnabled) {
    const failThreshold = optionalPositiveInteger(values.failThreshold);
    if (failThreshold === null) {
      return { errorKey: "invalidCircuitBreakerConfig", value: null };
    }

    const circuitBreaker: JsonRecord = {};
    if (failThreshold !== undefined) {
      circuitBreaker.fail_threshold = failThreshold;
    }

    const cooldown = values.cooldown.trim();
    if (cooldown) circuitBreaker.cooldown = cooldown;
    loadBalancer.circuit_breaker = circuitBreaker;
  }

  const upstream: JsonRecord = {
    targets,
    load_balancer: loadBalancer,
  };

  if (values.transportEnabled) {
    const transport: JsonRecord = {
      protocols: values.protocols,
    };
    const connectTimeout = values.connectTimeout.trim();
    if (connectTimeout) transport.connect_timeout = connectTimeout;
    upstream.transport = transport;
  }

  return { errorKey: null, value: upstream };
}

const UpstreamForm: React.FC<GatewayNamedFormProps> = ({
  canDelete,
  existingNames,
  selectedName,
  selectedValue,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const [values, setValues] = useState<FormValues>(() =>
    upstreamToFormValues(selectedName, selectedValue),
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setValues(upstreamToFormValues(selectedName, selectedValue));
    setError(null);
  }, [selectedName, selectedValue]);

  const setField = <Key extends keyof FormValues>(
    key: Key,
    value: FormValues[Key],
  ) => {
    setValues((current) => ({ ...current, [key]: value }));
  };

  const updateTarget = (id: string, patch: Partial<TargetValues>) => {
    setValues((current) => ({
      ...current,
      targets: current.targets.map((target) =>
        target.id === id ? { ...target, ...patch } : target,
      ),
    }));
  };

  const removeTarget = (id: string) => {
    setValues((current) => ({
      ...current,
      targets: current.targets.filter((target) => target.id !== id),
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

    const result = formValuesToUpstream(values);
    if (!result.value) {
      setError(t(result.errorKey ?? "invalidUpstreamConfig"));
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
          label={t("objectName")}
          value={values.name}
          onChange={(event) => setField("name", event.currentTarget.value)}
        />

        <Stack gap="xs">
          <Group justify="space-between">
            <Text fw={700}>{t("targets")}</Text>
            <Tooltip label={t("addTarget")} position="left" offset={10}>
              <ActionIcon
                size="sm"
                onClick={() =>
                  setField("targets", [...values.targets, createTarget()])
                }
              >
                <FaPlus />
              </ActionIcon>
            </Tooltip>
          </Group>
          {values.targets.map((target, index) => (
            <Box
              p="xs"
              key={target.id}
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
                  <TextInput
                    size="xs"
                    style={{ flex: 1 }}
                    variant="filled"
                    label={`${t("targetUrl")} ${index + 1}`}
                    value={target.url}
                    onChange={(event) =>
                      updateTarget(target.id, {
                        url: event.currentTarget.value,
                      })
                    }
                  />
                  <NumberInput
                    size="xs"
                    step={1}
                    min={1}
                    style={{ maxWidth: 80 }}
                    max={targetWeightMax}
                    allowDecimal={false}
                    variant="filled"
                    label={t("weight")}
                    value={target.weight}
                    onChange={(weight) => updateTarget(target.id, { weight })}
                  />
                </Group>
                <Tooltip label={t("delete")} position="left">
                  <ActionIcon
                    mt={25}
                    size="md"
                    type="button"
                    color="red"
                    variant="transparent"
                    disabled={values.targets.length === 1}
                    onClick={() => removeTarget(target.id)}
                  >
                    <IoCloseSharp />
                  </ActionIcon>
                </Tooltip>
              </Group>
            </Box>
          ))}
        </Stack>
      </Stack>

      <Divider size={3} />

      <Stack p="xs" gap="xs">
        <Text fw={700}>{t("loadBalancer")}</Text>
        <Select
          allowDeselect={false}
          variant="filled"
          label={t("strategy")}
          data={loadBalancerStrategyOptions}
          value={values.loadBalancerStrategy}
          onChange={(value) =>
            setField(
              "loadBalancerStrategy",
              (value ?? values.loadBalancerStrategy) as LoadBalancerStrategy,
            )
          }
        />
      </Stack>

      <Divider size={3} />

      <Stack p="xs" gap="xs">
        <Switch
          label={t("enableLivenessProbe")}
          checked={values.livenessEnabled}
          onChange={(event) =>
            setField("livenessEnabled", event.currentTarget.checked)
          }
        />
        {values.livenessEnabled && (
          <>
            <TextInput
              variant="filled"
              label={t("probePath")}
              value={values.livenessPath}
              onChange={(event) =>
                setField("livenessPath", event.currentTarget.value)
              }
            />
            <Group grow align="flex-start">
              <TextInput
                variant="filled"
                label={t("interval")}
                value={values.livenessInterval}
                onChange={(event) =>
                  setField("livenessInterval", event.currentTarget.value)
                }
              />
              <TextInput
                variant="filled"
                label={t("timeout")}
                value={values.livenessTimeout}
                onChange={(event) =>
                  setField("livenessTimeout", event.currentTarget.value)
                }
              />
            </Group>
          </>
        )}
      </Stack>

      <Divider size={3} />

      <Stack p="xs" gap="xs">
        <Switch
          label={t("enableCircuitBreaker")}
          checked={values.circuitBreakerEnabled}
          onChange={(event) =>
            setField("circuitBreakerEnabled", event.currentTarget.checked)
          }
        />
        {values.circuitBreakerEnabled && (
          <Group grow align="flex-start">
            <NumberInput
              min={1}
              step={1}
              allowDecimal={false}
              variant="filled"
              label={t("failThreshold")}
              value={values.failThreshold}
              onChange={(value) => setField("failThreshold", value)}
            />
            <TextInput
              variant="filled"
              label={t("cooldown")}
              value={values.cooldown}
              onChange={(event) =>
                setField("cooldown", event.currentTarget.value)
              }
            />
          </Group>
        )}
      </Stack>

      <Divider size={3} />

      <Stack p="xs" gap="xs">
        <Switch
          label={t("enableTransport")}
          checked={values.transportEnabled}
          onChange={(event) =>
            setField("transportEnabled", event.currentTarget.checked)
          }
        />
        {values.transportEnabled && (
          <>
            <TextInput
              variant="filled"
              label={t("connectTimeout")}
              value={values.connectTimeout}
              onChange={(event) =>
                setField("connectTimeout", event.currentTarget.value)
              }
            />
            <Checkbox.Group
              label={t("protocols")}
              value={values.protocols}
              onChange={(protocols) =>
                setField("protocols", protocols.filter(isUpstreamProtocol))
              }
            >
              <Group mt="xs">
                {upstreamProtocolOptions.map((protocol) => (
                  <Checkbox
                    key={protocol.value}
                    value={protocol.value}
                    label={protocol.label}
                  />
                ))}
              </Group>
            </Checkbox.Group>
          </>
        )}
      </Stack>
    </GatewayEntityForm>
  );
};

export default UpstreamForm;
