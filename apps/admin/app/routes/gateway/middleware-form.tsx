import { useEffect, useState } from "react";
import {
  Divider,
  Select,
  Stack,
  TagsInput,
  Text,
  TextInput,
} from "@mantine/core";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import { isRecord, type JsonRecord } from "./config-utils";
import GatewayEntityForm from "./gateway-entity-form";
import {
  getObjectNameErrorKey,
  type GatewayNamedFormProps,
} from "./gateway-form-utils";
import HeaderValueRows, { type HeaderValueRow } from "./header-value-rows";
import {
  isMiddlewareKind,
  middlewareKindOptions,
  type MiddlewareKind,
} from "./schema-options";

interface FormValues {
  name: string;
  kind: MiddlewareKind;
  prefixes: string[];
  prefix: string;
  pattern: string;
  replacement: string;
  requestAdd: HeaderValueRow[];
  requestSet: HeaderValueRow[];
  requestRemove: string[];
  responseAdd: HeaderValueRow[];
  responseSet: HeaderValueRow[];
  responseRemove: string[];
}

interface HeaderTransformValues {
  add: HeaderValueRow[];
  set: HeaderValueRow[];
  remove: string[];
}

interface BuildResult {
  errorKey: string | null;
  value: JsonValue | null;
}

let rowId = 0;

function createHeader(name = "", value = ""): HeaderValueRow {
  rowId += 1;
  return {
    id: `middleware-header-${rowId}`,
    name,
    value,
  };
}

function defaultFormValues(selectedName: string): FormValues {
  return {
    name: selectedName,
    kind: "strip_prefix",
    prefixes: ["/api"],
    prefix: "/api",
    pattern: "^/api/(.*)$",
    replacement: "/$1",
    requestAdd: [],
    requestSet: [],
    requestRemove: [],
    responseAdd: [],
    responseSet: [],
    responseRemove: [],
  };
}

function stringsToForm(value: JsonValue | undefined) {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function headersToForm(value: JsonValue | undefined) {
  if (!Array.isArray(value)) return [];

  return value.flatMap((item) => {
    if (!isRecord(item)) return [];
    return [
      createHeader(
        typeof item.name === "string" ? item.name : "",
        typeof item.value === "string" ? item.value : "",
      ),
    ];
  });
}

function middlewareToFormValues(
  selectedName: string,
  selectedValue: JsonValue | null,
): FormValues {
  const defaults = defaultFormValues(selectedName);
  if (!isRecord(selectedValue)) return defaults;

  const kind = isMiddlewareKind(selectedValue.kind)
    ? selectedValue.kind
    : defaults.kind;

  return {
    ...defaults,
    name: selectedName,
    kind,
    prefixes: stringsToForm(selectedValue.prefixes),
    prefix:
      typeof selectedValue.prefix === "string"
        ? selectedValue.prefix
        : defaults.prefix,
    pattern:
      typeof selectedValue.pattern === "string"
        ? selectedValue.pattern
        : defaults.pattern,
    replacement:
      typeof selectedValue.replacement === "string"
        ? selectedValue.replacement
        : defaults.replacement,
    requestAdd: headersToForm(selectedValue.add),
    requestSet: headersToForm(selectedValue.set),
    requestRemove: stringsToForm(selectedValue.remove),
    responseAdd: headersToForm(selectedValue.add),
    responseSet: headersToForm(selectedValue.set),
    responseRemove: stringsToForm(selectedValue.remove),
  };
}

function cleanStrings(values: string[]) {
  return values.map((value) => value.trim()).filter(Boolean);
}

function buildHeaderRows(rows: HeaderValueRow[]) {
  const headers: JsonRecord[] = [];

  for (const row of rows) {
    const name = row.name.trim();
    if (!name && !row.value.trim()) continue;
    if (!name) return null;
    headers.push({ name, value: row.value });
  }

  return headers;
}

function buildHeaderTransform(
  values: HeaderTransformValues,
): JsonRecord | null {
  const add = buildHeaderRows(values.add);
  const set = buildHeaderRows(values.set);
  if (!add || !set) return null;

  return {
    add,
    set,
    remove: cleanStrings(values.remove),
  };
}

function formValuesToMiddleware(values: FormValues): BuildResult {
  switch (values.kind) {
    case "strip_prefix": {
      const prefixes = cleanStrings(values.prefixes);
      if (prefixes.length === 0) {
        return { errorKey: "prefixesRequired", value: null };
      }

      return {
        errorKey: null,
        value: {
          kind: "strip_prefix",
          prefixes,
        },
      };
    }
    case "add_prefix": {
      const prefix = values.prefix.trim();
      if (!prefix) return { errorKey: "prefixRequired", value: null };

      return {
        errorKey: null,
        value: {
          kind: "add_prefix",
          prefix,
        },
      };
    }
    case "replace_path_regex": {
      const pattern = values.pattern.trim();
      if (!pattern) return { errorKey: "regexPatternRequired", value: null };

      return {
        errorKey: null,
        value: {
          kind: "replace_path_regex",
          pattern,
          replacement: values.replacement,
        },
      };
    }
    case "preserve_host":
      return {
        errorKey: null,
        value: {
          kind: "preserve_host",
        },
      };
    case "request_headers": {
      const transform = buildHeaderTransform({
        add: values.requestAdd,
        set: values.requestSet,
        remove: values.requestRemove,
      });
      if (!transform) return { errorKey: "headerNameRequired", value: null };

      return {
        errorKey: null,
        value: {
          kind: "request_headers",
          ...transform,
        },
      };
    }
    case "response_headers": {
      const transform = buildHeaderTransform({
        add: values.responseAdd,
        set: values.responseSet,
        remove: values.responseRemove,
      });
      if (!transform) return { errorKey: "headerNameRequired", value: null };

      return {
        errorKey: null,
        value: {
          kind: "response_headers",
          ...transform,
        },
      };
    }
  }
}

const MiddlewareForm: React.FC<GatewayNamedFormProps> = ({
  canDelete,
  existingNames,
  selectedName,
  selectedValue,
  onApply,
  onDelete,
}) => {
  const t = useTranslations();
  const [values, setValues] = useState<FormValues>(() =>
    middlewareToFormValues(selectedName, selectedValue),
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setValues(middlewareToFormValues(selectedName, selectedValue));
    setError(null);
  }, [selectedName, selectedValue]);

  const setField = <Key extends keyof FormValues>(
    key: Key,
    value: FormValues[Key],
  ) => {
    setValues((current) => ({ ...current, [key]: value }));
  };

  const updateRequestAdd = (id: string, patch: Partial<HeaderValueRow>) => {
    setValues((current) => ({
      ...current,
      requestAdd: current.requestAdd.map((header) =>
        header.id === id ? { ...header, ...patch } : header,
      ),
    }));
  };

  const updateRequestSet = (id: string, patch: Partial<HeaderValueRow>) => {
    setValues((current) => ({
      ...current,
      requestSet: current.requestSet.map((header) =>
        header.id === id ? { ...header, ...patch } : header,
      ),
    }));
  };

  const updateResponseAdd = (id: string, patch: Partial<HeaderValueRow>) => {
    setValues((current) => ({
      ...current,
      responseAdd: current.responseAdd.map((header) =>
        header.id === id ? { ...header, ...patch } : header,
      ),
    }));
  };

  const updateResponseSet = (id: string, patch: Partial<HeaderValueRow>) => {
    setValues((current) => ({
      ...current,
      responseSet: current.responseSet.map((header) =>
        header.id === id ? { ...header, ...patch } : header,
      ),
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

    const result = formValuesToMiddleware(values);
    if (!result.value) {
      setError(t(result.errorKey ?? "invalidMiddlewareConfig"));
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
          data={middlewareKindOptions}
          value={values.kind}
          onChange={(kind) =>
            setField("kind", (kind ?? values.kind) as MiddlewareKind)
          }
        />
      </Stack>

      <Divider size={3} />
      <Stack p="xs" gap="xs">
        {values.kind === "strip_prefix" && (
          <TagsInput
            variant="filled"
            label={t("prefixes")}
            value={values.prefixes}
            onChange={(prefixes) => setField("prefixes", prefixes)}
          />
        )}

        {values.kind === "add_prefix" && (
          <TextInput
            variant="filled"
            label={t("prefix")}
            value={values.prefix}
            onChange={(event) => setField("prefix", event.currentTarget.value)}
          />
        )}

        {values.kind === "replace_path_regex" && (
          <Stack gap="xs">
            <TextInput
              variant="filled"
              label={t("pattern")}
              value={values.pattern}
              onChange={(event) =>
                setField("pattern", event.currentTarget.value)
              }
            />
            <TextInput
              variant="filled"
              label={t("replacement")}
              value={values.replacement}
              onChange={(event) =>
                setField("replacement", event.currentTarget.value)
              }
            />
          </Stack>
        )}

        {values.kind === "preserve_host" && (
          <Text size="sm" c="dimmed">
            {t("noMiddlewareSettings")}
          </Text>
        )}

        {values.kind === "request_headers" && (
          <Stack gap="sm">
            <HeaderValueRows
              rows={values.requestAdd}
              titleKey="headersToAdd"
              onAdd={() =>
                setField("requestAdd", [...values.requestAdd, createHeader()])
              }
              onRemove={(id) =>
                setField(
                  "requestAdd",
                  values.requestAdd.filter((header) => header.id !== id),
                )
              }
              onUpdate={updateRequestAdd}
            />
            <HeaderValueRows
              rows={values.requestSet}
              titleKey="headersToSet"
              onAdd={() =>
                setField("requestSet", [...values.requestSet, createHeader()])
              }
              onRemove={(id) =>
                setField(
                  "requestSet",
                  values.requestSet.filter((header) => header.id !== id),
                )
              }
              onUpdate={updateRequestSet}
            />
            <TagsInput
              variant="filled"
              label={t("headersToRemove")}
              value={values.requestRemove}
              onChange={(remove) => setField("requestRemove", remove)}
            />
          </Stack>
        )}

        {values.kind === "response_headers" && (
          <Stack gap="sm">
            <HeaderValueRows
              rows={values.responseAdd}
              titleKey="headersToAdd"
              onAdd={() =>
                setField("responseAdd", [...values.responseAdd, createHeader()])
              }
              onRemove={(id) =>
                setField(
                  "responseAdd",
                  values.responseAdd.filter((header) => header.id !== id),
                )
              }
              onUpdate={updateResponseAdd}
            />
            <HeaderValueRows
              rows={values.responseSet}
              titleKey="headersToSet"
              onAdd={() =>
                setField("responseSet", [...values.responseSet, createHeader()])
              }
              onRemove={(id) =>
                setField(
                  "responseSet",
                  values.responseSet.filter((header) => header.id !== id),
                )
              }
              onUpdate={updateResponseSet}
            />
            <TagsInput
              variant="filled"
              label={t("headersToRemove")}
              value={values.responseRemove}
              onChange={(remove) => setField("responseRemove", remove)}
            />
          </Stack>
        )}
      </Stack>
    </GatewayEntityForm>
  );
};

export default MiddlewareForm;
