import { useEffect, useState } from "react";
import { Group, NumberInput, Select, Stack, TextInput } from "@mantine/core";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import { isRecord } from "./config-utils";
import GatewayEntityForm from "./gateway-entity-form";
import {
  getObjectNameErrorKey,
  positiveInteger,
  type GatewayNamedFormProps,
} from "./gateway-form-utils";
import {
  isCalendarPeriod,
  isLimitPeriod,
  isLimitStrategy,
  limitPeriodOptions,
  limitStrategyOptions,
  type LimitPeriod,
  type LimitStrategy,
} from "./schema-options";

interface FormValues {
  name: string;
  strategy: LimitStrategy;
  maxBurst: number | string;
  replenishOnePer: string;
  capacity: number | string;
  refillRate: number | string;
  limit: number | string;
  period: LimitPeriod;
  timezone: string;
  anchorDay: number | string;
}

const defaultValues: FormValues = {
  name: "",
  strategy: "gcra",
  maxBurst: 100,
  replenishOnePer: "1s",
  capacity: 100,
  refillRate: 10,
  limit: 1000,
  period: "day",
  timezone: "UTC",
  anchorDay: "",
};

function limitToFormValues(
  selectedName: string,
  selectedValue: JsonValue | null,
): FormValues {
  if (!isRecord(selectedValue)) {
    return { ...defaultValues, name: selectedName };
  }

  const params = isRecord(selectedValue.params) ? selectedValue.params : {};
  const strategy = isLimitStrategy(selectedValue.strategy)
    ? selectedValue.strategy
    : "gcra";

  return {
    ...defaultValues,
    name: selectedName,
    strategy,
    maxBurst:
      typeof params.max_burst === "number"
        ? params.max_burst
        : defaultValues.maxBurst,
    replenishOnePer:
      typeof params.replenish_1_per === "string"
        ? params.replenish_1_per
        : defaultValues.replenishOnePer,
    capacity:
      typeof params.capacity === "number"
        ? params.capacity
        : defaultValues.capacity,
    refillRate:
      typeof params.refill_rate === "number"
        ? params.refill_rate
        : defaultValues.refillRate,
    limit:
      typeof params.limit === "number" ? params.limit : defaultValues.limit,
    period: isLimitPeriod(params.period) ? params.period : defaultValues.period,
    timezone:
      typeof params.timezone === "string"
        ? params.timezone
        : defaultValues.timezone,
    anchorDay:
      typeof params.anchor_day === "number"
        ? params.anchor_day
        : defaultValues.anchorDay,
  };
}

function formValuesToLimit(values: FormValues): JsonValue | null {
  switch (values.strategy) {
    case "gcra": {
      const maxBurst = positiveInteger(values.maxBurst);
      if (!maxBurst || !values.replenishOnePer.trim()) return null;

      return {
        strategy: "gcra",
        params: {
          max_burst: maxBurst,
          replenish_1_per: values.replenishOnePer.trim(),
        },
      };
    }
    case "token_bucket": {
      const capacity = positiveInteger(values.capacity);
      const refillRate = positiveInteger(values.refillRate);
      if (!capacity || !refillRate) return null;

      return {
        strategy: "token_bucket",
        params: {
          capacity,
          refill_rate: refillRate,
        },
      };
    }
    case "quota_tracker": {
      const limit = positiveInteger(values.limit);
      if (!limit) return null;

      const params: Record<string, JsonValue> = {
        limit,
        period: values.period,
      };
      if (isCalendarPeriod(values.period)) {
        const timezone = values.timezone.trim();
        if (!timezone) return null;
        params.timezone = timezone;
        if (values.period === "month" && values.anchorDay !== "") {
          const anchorDay = positiveInteger(values.anchorDay);
          if (!anchorDay || anchorDay > 28) return null;
          params.anchor_day = anchorDay;
        }
      }

      return {
        strategy: "quota_tracker",
        params,
      };
    }
  }
}

const LimitForm: React.FC<GatewayNamedFormProps> = ({
  canDelete,
  existingNames,
  selectedName,
  selectedValue,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const [values, setValues] = useState<FormValues>(() =>
    limitToFormValues(selectedName, selectedValue),
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setValues(limitToFormValues(selectedName, selectedValue));
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

    const limit = formValuesToLimit(values);
    if (!limit) {
      setError(t("invalidLimitConfig"));
      return;
    }

    setError(null);
    onApply(selectedName, trimmedName, limit);
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
          withAlignedLabels
          allowDeselect={false}
          variant="filled"
          label={t("strategy")}
          data={limitStrategyOptions}
          value={values.strategy}
          onChange={(value) =>
            setField("strategy", (value ?? values.strategy) as LimitStrategy)
          }
        />

        {values.strategy === "gcra" && (
          <Group grow align="flex-start">
            <NumberInput
              min={1}
              step={1}
              allowDecimal={false}
              variant="filled"
              label={t("maxBurst")}
              value={values.maxBurst}
              onChange={(value) => setField("maxBurst", value)}
            />
            <TextInput
              variant="filled"
              label={t("replenishOnePer")}
              value={values.replenishOnePer}
              onChange={(event) =>
                setField("replenishOnePer", event.currentTarget.value)
              }
            />
          </Group>
        )}

        {values.strategy === "token_bucket" && (
          <Group grow align="flex-start">
            <NumberInput
              min={1}
              step={1}
              allowDecimal={false}
              variant="filled"
              label={t("capacity")}
              value={values.capacity}
              onChange={(value) => setField("capacity", value)}
            />
            <NumberInput
              min={1}
              step={1}
              allowDecimal={false}
              variant="filled"
              label={t("refillRate")}
              value={values.refillRate}
              onChange={(value) => setField("refillRate", value)}
            />
          </Group>
        )}

        {values.strategy === "quota_tracker" && (
          <Stack gap="xs">
            <Group grow align="flex-start">
              <NumberInput
                min={1}
                step={1}
                allowDecimal={false}
                variant="filled"
                label={t("limit")}
                value={values.limit}
                onChange={(value) => setField("limit", value)}
              />
              <Select
                withAlignedLabels
                allowDeselect={false}
                variant="filled"
                label={t("period")}
                data={limitPeriodOptions}
                value={values.period}
                onChange={(value) =>
                  setField("period", (value ?? values.period) as LimitPeriod)
                }
              />
            </Group>
            {isCalendarPeriod(values.period) && (
              <Group grow align="flex-start">
                <TextInput
                  variant="filled"
                  label={t("timezone")}
                  value={values.timezone}
                  onChange={(event) =>
                    setField("timezone", event.currentTarget.value)
                  }
                />
                {values.period === "month" && (
                  <NumberInput
                    min={1}
                    max={28}
                    step={1}
                    allowDecimal={false}
                    variant="filled"
                    label={t("anchorDay")}
                    value={values.anchorDay}
                    onChange={(value) => setField("anchorDay", value)}
                  />
                )}
              </Group>
            )}
          </Stack>
        )}
      </Stack>
    </GatewayEntityForm>
  );
};

export default LimitForm;
