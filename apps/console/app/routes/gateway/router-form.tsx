import { useEffect, useMemo, useState } from "react";
import {
  Divider,
  MultiSelect,
  NumberInput,
  Select,
  Stack,
  TextInput,
} from "@mantine/core";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import { isRecord, type JsonRecord } from "./config-utils";
import GatewayEntityForm from "./gateway-entity-form";
import {
  getObjectNameErrorKey,
  nameOptions,
  positiveInteger,
  type GatewayNamedFormProps,
} from "./gateway-form-utils";
import MatchExpressionEditor from "./match-expression-editor";
import {
  formValuesToMatch,
  matchToFormValues,
  type MatchValues,
} from "./match-expression-utils";

interface Props extends GatewayNamedFormProps {
  middlewareNames: string[];
  policyNames: string[];
  serviceNames: string[];
}

interface FormValues {
  name: string;
  priority: number | string;
  matcher: MatchValues;
  service: string;
  middlewares: string[];
  policies: string[];
  quotaCost: number | string;
  responseMode: "finite" | "stream";
}

interface BuildContext {
  middlewareNames: string[];
  policyNames: string[];
  serviceNames: string[];
}

interface BuildResult {
  errorKey: string | null;
  value: JsonValue | null;
}

function stringsToForm(value: JsonValue | undefined) {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function routerToFormValues(
  selectedName: string,
  selectedValue: JsonValue | null,
): FormValues {
  if (!isRecord(selectedValue)) {
    return {
      name: selectedName,
      priority: 100,
      matcher: matchToFormValues(null),
      service: "",
      middlewares: [],
      policies: [],
      quotaCost: "",
      responseMode: "finite",
    };
  }

  return {
    name: selectedName,
    priority:
      typeof selectedValue.priority === "number" ? selectedValue.priority : "",
    matcher: matchToFormValues(selectedValue.match),
    service:
      typeof selectedValue.service === "string" ? selectedValue.service : "",
    middlewares: stringsToForm(selectedValue.middlewares),
    policies: stringsToForm(selectedValue.policies),
    responseMode:
      selectedValue.response_mode === "stream" ? "stream" : "finite",
    quotaCost:
      typeof selectedValue.quota_cost === "number"
        ? selectedValue.quota_cost
        : "",
  };
}

function optionalInteger(value: number | string) {
  if (value === "") return undefined;
  const numberValue = Number(value);
  return Number.isInteger(numberValue) ? numberValue : null;
}

function hasUnknownReference(values: string[], names: string[]) {
  return values.some((value) => !names.includes(value));
}

function formValuesToRouter(
  values: FormValues,
  context: BuildContext,
): BuildResult {
  const service = values.service.trim();
  if (!service) return { errorKey: "serviceReferenceRequired", value: null };
  if (!context.serviceNames.includes(service)) {
    return { errorKey: "unknownServiceReference", value: null };
  }

  if (hasUnknownReference(values.middlewares, context.middlewareNames)) {
    return { errorKey: "unknownMiddlewareReference", value: null };
  }

  if (hasUnknownReference(values.policies, context.policyNames)) {
    return { errorKey: "unknownPolicyReference", value: null };
  }

  const priority = optionalInteger(values.priority);
  if (priority === null) return { errorKey: "invalidPriority", value: null };

  const match = formValuesToMatch(values.matcher);
  if (!match.value) return match;

  const router: JsonRecord = {
    response_mode: values.responseMode,
    match: match.value,
    service,
    middlewares: values.middlewares,
    policies: values.policies,
  };
  if (priority !== undefined) router.priority = priority;

  const quotaCost = positiveInteger(values.quotaCost);
  if (quotaCost) router.quota_cost = quotaCost;

  return { errorKey: null, value: router };
}

const RouterForm: React.FC<Props> = ({
  canDelete,
  existingNames,
  middlewareNames,
  policyNames,
  selectedName,
  selectedValue,
  serviceNames,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const [values, setValues] = useState<FormValues>(() =>
    routerToFormValues(selectedName, selectedValue),
  );
  const [error, setError] = useState<string | null>(null);
  const serviceOptions = useMemo(
    () => nameOptions(serviceNames),
    [serviceNames],
  );
  const middlewareOptions = useMemo(
    () => nameOptions(middlewareNames),
    [middlewareNames],
  );
  const policyOptions = useMemo(() => nameOptions(policyNames), [policyNames]);

  useEffect(() => {
    setValues(routerToFormValues(selectedName, selectedValue));
    setError(null);
  }, [selectedName, selectedValue]);

  const setField = <Key extends keyof FormValues>(
    key: Key,
    value: FormValues[Key],
  ) => {
    setValues((current) => ({ ...current, [key]: value }));
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

    const result = formValuesToRouter(values, {
      middlewareNames,
      policyNames,
      serviceNames,
    });
    if (!result.value) {
      setError(t(result.errorKey ?? "invalidRouterConfig"));
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
        <NumberInput
          step={1}
          allowDecimal={false}
          variant="filled"
          label={t("priority")}
          value={values.priority}
          onChange={(priority) => setField("priority", priority)}
        />
        <Select
          variant="filled"
          label={t("responseMode")}
          description={t("responseModeDescription")}
          value={values.responseMode}
          data={[
            { value: "finite", label: t("responseModeFinite") },
            { value: "stream", label: t("responseModeStream") },
          ]}
          onChange={(mode) =>
            setField("responseMode", mode === "stream" ? "stream" : "finite")
          }
        />
        <NumberInput
          min={1}
          step={1}
          allowDecimal={false}
          variant="filled"
          label={t("quotaCost")}
          value={values.quotaCost}
          onChange={(cost) => setField("quotaCost", cost)}
        />
        <Select
          searchable
          variant="filled"
          label={t("service")}
          placeholder={t("selectService")}
          data={serviceOptions}
          value={values.service || null}
          onChange={(service) => setField("service", service ?? "")}
        />
        <MultiSelect
          searchable
          clearable
          variant="filled"
          label={t("middlewares")}
          data={middlewareOptions}
          value={values.middlewares}
          onChange={(middlewares) => setField("middlewares", middlewares)}
        />
        <MultiSelect
          searchable
          clearable
          variant="filled"
          label={t("policies")}
          data={policyOptions}
          value={values.policies}
          onChange={(policies) => setField("policies", policies)}
        />
      </Stack>
      <Divider size={3} />

      <Stack p="xs" gap="xs">
        <MatchExpressionEditor
          value={values.matcher}
          onChange={(matcher) => setField("matcher", matcher)}
        />
      </Stack>
    </GatewayEntityForm>
  );
};

export default RouterForm;
