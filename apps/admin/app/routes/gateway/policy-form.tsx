import { useEffect, useMemo, useState } from "react";
import {
  Checkbox,
  Divider,
  Group,
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
import {
  authStrategyOptions,
  envProfileOptions,
  isAuthStrategy,
  isEnvProfile,
  isPolicyKind,
  policyKindOptions,
  type AuthStrategy,
  type EnvProfile,
  type PolicyKind,
} from "./schema-options";

interface Props extends GatewayNamedFormProps {
  limitNames: string[];
}

interface FormValues {
  name: string;
  kind: PolicyKind;
  authStrategies: AuthStrategy[];
  resource: string;
  env: EnvProfile;
  rateLimit: string;
  quotaLimit: string;
  quotaCost: number | string;
}

interface BuildContext {
  limitNames: string[];
}

interface BuildResult {
  errorKey: string | null;
  value: JsonValue | null;
}

const defaultValues: FormValues = {
  name: "",
  kind: "auth",
  authStrategies: ["jwt"],
  resource: "",
  env: "none",
  rateLimit: "",
  quotaLimit: "",
  quotaCost: 1,
};

function policyToFormValues(
  selectedName: string,
  selectedValue: JsonValue | null,
): FormValues {
  if (!isRecord(selectedValue)) {
    return { ...defaultValues, name: selectedName };
  }

  return {
    ...defaultValues,
    name: selectedName,
    kind: isPolicyKind(selectedValue.kind) ? selectedValue.kind : "auth",
    authStrategies: Array.isArray(selectedValue.strategies)
      ? selectedValue.strategies.filter(isAuthStrategy)
      : defaultValues.authStrategies,
    resource:
      typeof selectedValue.resource === "string" ? selectedValue.resource : "",
    env: isEnvProfile(selectedValue.env) ? selectedValue.env : "none",
    rateLimit:
      typeof selectedValue.limit === "string" ? selectedValue.limit : "",
    quotaLimit:
      typeof selectedValue.limit === "string" ? selectedValue.limit : "",
    quotaCost:
      typeof selectedValue.cost === "number"
        ? selectedValue.cost
        : defaultValues.quotaCost,
  };
}

function validateLimitReference(value: string, limitNames: string[]) {
  if (!value) return "limitReferenceRequired";
  if (!limitNames.includes(value)) return "unknownLimitReference";
  return null;
}

function formValuesToPolicy(
  values: FormValues,
  context: BuildContext,
): BuildResult {
  switch (values.kind) {
    case "auth":
      if (values.authStrategies.length === 0) {
        return { errorKey: "authStrategiesRequired", value: null };
      }

      return {
        errorKey: null,
        value: {
          kind: "auth",
          strategies: values.authStrategies,
        },
      };
    case "access_control": {
      const resource = values.resource.trim();
      if (!resource) return { errorKey: "resourceRequired", value: null };

      const policy: JsonRecord = {
        kind: "access_control",
        resource,
      };
      if (values.env !== "none") policy.env = values.env;

      return {
        errorKey: null,
        value: policy,
      };
    }
    case "rate_limit": {
      const limit = values.rateLimit.trim();
      const limitError = validateLimitReference(limit, context.limitNames);
      if (limitError) return { errorKey: limitError, value: null };

      return {
        errorKey: null,
        value: {
          kind: "rate_limit",
          limit,
        },
      };
    }
    case "quota": {
      const limit = values.quotaLimit.trim();
      const limitError = validateLimitReference(limit, context.limitNames);
      if (limitError) return { errorKey: limitError, value: null };

      const cost = positiveInteger(values.quotaCost);
      if (!cost) return { errorKey: "quotaCostRequired", value: null };

      return {
        errorKey: null,
        value: {
          kind: "quota",
          limit,
          cost,
        },
      };
    }
  }
}

const PolicyForm: React.FC<Props> = ({
  canDelete,
  existingNames,
  limitNames,
  selectedName,
  selectedValue,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const [values, setValues] = useState<FormValues>(() =>
    policyToFormValues(selectedName, selectedValue),
  );
  const [error, setError] = useState<string | null>(null);
  const limitOptions = useMemo(() => nameOptions(limitNames), [limitNames]);

  useEffect(() => {
    setValues(policyToFormValues(selectedName, selectedValue));
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

    const result = formValuesToPolicy(values, { limitNames });
    if (!result.value) {
      setError(t(result.errorKey ?? "invalidPolicyConfig"));
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
        <Select
          allowDeselect={false}
          variant="filled"
          label={t("kind")}
          data={policyKindOptions}
          value={values.kind}
          onChange={(kind) =>
            setField("kind", (kind ?? values.kind) as PolicyKind)
          }
        />
      </Stack>

      <Divider size={3} />

      <Stack p="xs" gap="xs">
        {values.kind === "auth" && (
          <Checkbox.Group
            label={t("authStrategies")}
            value={values.authStrategies}
            onChange={(strategies) =>
              setField("authStrategies", strategies.filter(isAuthStrategy))
            }
          >
            <Stack mt="xs" gap="xs">
              {authStrategyOptions.map((strategy) => (
                <Checkbox
                  key={strategy.value}
                  value={strategy.value}
                  label={strategy.label}
                />
              ))}
            </Stack>
          </Checkbox.Group>
        )}

        {values.kind === "access_control" && (
          <Stack gap="xs">
            <TextInput
              variant="filled"
              label={t("resource")}
              value={values.resource}
              onChange={(event) =>
                setField("resource", event.currentTarget.value)
              }
            />
            <Select
              allowDeselect={false}
              variant="filled"
              label={t("envProfile")}
              data={envProfileOptions}
              value={values.env}
              onChange={(env) =>
                setField("env", (env ?? values.env) as EnvProfile)
              }
            />
          </Stack>
        )}

        {values.kind === "rate_limit" && (
          <Select
            searchable
            variant="filled"
            label={t("limit")}
            placeholder={t("selectLimit")}
            data={limitOptions}
            value={values.rateLimit || null}
            onChange={(limit) => setField("rateLimit", limit ?? "")}
          />
        )}

        {values.kind === "quota" && (
          <Group grow align="flex-start">
            <Select
              searchable
              variant="filled"
              label={t("limit")}
              placeholder={t("selectLimit")}
              data={limitOptions}
              value={values.quotaLimit || null}
              onChange={(limit) => setField("quotaLimit", limit ?? "")}
            />
            <NumberInput
              min={1}
              step={1}
              allowDecimal={false}
              variant="filled"
              label={t("cost")}
              value={values.quotaCost}
              onChange={(cost) => setField("quotaCost", cost)}
            />
          </Group>
        )}
      </Stack>
    </GatewayEntityForm>
  );
};

export default PolicyForm;
