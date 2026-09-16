import type { JsonValue } from "@/services/types";
import { isRecord } from "./config-utils";
import {
  isMatchKind,
  pathOperatorOptions,
  valueOperatorOptions,
  type MatchKind,
  type PathOperator,
  type ValueOperator,
} from "./schema-options";

export interface MatchValues {
  id: string;
  kind: MatchKind;
  pathOperator: PathOperator;
  pathValue: string;
  methods: string[];
  name: string;
  valueOperator: ValueOperator;
  valueText: string;
  valueValues: string[];
  present: boolean;
  cidrs: string[];
  children: MatchValues[];
}

interface BuildResult {
  errorKey: string | null;
  value: JsonValue | null;
}

let matchId = 0;

export function createMatchValues(kind: MatchKind = "path"): MatchValues {
  matchId += 1;
  return {
    id: `match-${matchId}`,
    kind,
    pathOperator: "prefix",
    pathValue: "/",
    methods: ["GET"],
    name: "",
    valueOperator: "eq",
    valueText: "",
    valueValues: [],
    present: true,
    cidrs: [],
    children: [],
  };
}

function stringsToForm(value: JsonValue | undefined) {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function cleanStrings(values: string[]) {
  return values.map((value) => value.trim()).filter(Boolean);
}

function valueMatchToForm(value: JsonValue | undefined) {
  const fallback = {
    valueOperator: "eq" as ValueOperator,
    valueText: "",
    valueValues: [],
    present: true,
  };

  if (!isRecord(value)) return fallback;

  for (const option of valueOperatorOptions) {
    const raw = value[option.value];
    if (raw === undefined) continue;

    if (option.value === "present") {
      return {
        ...fallback,
        valueOperator: option.value,
        present: typeof raw === "boolean" ? raw : true,
      };
    }

    if (option.value === "one_of") {
      return {
        ...fallback,
        valueOperator: option.value,
        valueValues: stringsToForm(raw),
      };
    }

    return {
      ...fallback,
      valueOperator: option.value,
      valueText: typeof raw === "string" ? raw : "",
    };
  }

  return fallback;
}

function pathMatchToForm(value: JsonValue | undefined) {
  const fallback = {
    pathOperator: "prefix" as PathOperator,
    pathValue: "/",
  };

  if (!isRecord(value)) return fallback;

  for (const option of pathOperatorOptions) {
    const raw = value[option.value];
    if (typeof raw === "string") {
      return {
        pathOperator: option.value,
        pathValue: raw,
      };
    }
  }

  return fallback;
}

export function matchToFormValues(
  value: JsonValue | undefined | null,
): MatchValues {
  if (!isRecord(value)) return createMatchValues();

  const key = Object.keys(value).find(isMatchKind);
  if (!key) return createMatchValues();

  const match = createMatchValues(key);
  const raw = value[key];

  if (key === "all" || key === "any") {
    match.children = Array.isArray(raw)
      ? raw.map(matchToFormValues)
      : [createMatchValues()];
    return match;
  }

  if (key === "not") {
    match.children = [matchToFormValues(raw)];
    return match;
  }

  if (key === "method") {
    match.methods = stringsToForm(raw);
    return match;
  }

  if (key === "path") {
    return {
      ...match,
      ...pathMatchToForm(raw),
    };
  }

  if (key === "source_ip") {
    match.cidrs = isRecord(raw) ? stringsToForm(raw.cidrs) : [];
    return match;
  }

  if (key === "host") {
    return {
      ...match,
      ...valueMatchToForm(raw),
    };
  }

  if (key === "header" || key === "query" || key === "cookie") {
    return {
      ...match,
      name: isRecord(raw) && typeof raw.name === "string" ? raw.name : "",
      ...valueMatchToForm(raw),
    };
  }

  return match;
}

function buildValuePredicate(values: MatchValues): BuildResult {
  if (values.valueOperator === "present") {
    return {
      errorKey: null,
      value: { present: values.present },
    };
  }

  if (values.valueOperator === "one_of") {
    const valuesList = cleanStrings(values.valueValues);
    if (valuesList.length === 0) {
      return { errorKey: "matchValuesRequired", value: null };
    }

    return {
      errorKey: null,
      value: { one_of: valuesList },
    };
  }

  const value = values.valueText.trim();
  if (!value) return { errorKey: "matchValueRequired", value: null };

  return {
    errorKey: null,
    value: { [values.valueOperator]: value },
  };
}

export function formValuesToMatch(values: MatchValues): BuildResult {
  switch (values.kind) {
    case "all":
    case "any": {
      if (values.children.length === 0) {
        return { errorKey: "conditionsRequired", value: null };
      }

      const children: JsonValue[] = [];
      for (const child of values.children) {
        const result = formValuesToMatch(child);
        if (!result.value) return result;
        children.push(result.value);
      }

      return {
        errorKey: null,
        value: { [values.kind]: children },
      };
    }
    case "not": {
      const child = values.children[0];
      if (!child) return { errorKey: "childConditionRequired", value: null };

      const result = formValuesToMatch(child);
      if (!result.value) return result;

      return {
        errorKey: null,
        value: { not: result.value },
      };
    }
    case "path": {
      const pathValue = values.pathValue.trim();
      if (!pathValue) return { errorKey: "matchValueRequired", value: null };

      return {
        errorKey: null,
        value: {
          path: {
            [values.pathOperator]: pathValue,
          },
        },
      };
    }
    case "method": {
      const methods = cleanStrings(values.methods).map((method) =>
        method.toUpperCase(),
      );
      if (methods.length === 0) {
        return { errorKey: "methodsRequired", value: null };
      }

      return {
        errorKey: null,
        value: { method: methods },
      };
    }
    case "source_ip": {
      const cidrs = cleanStrings(values.cidrs);
      if (cidrs.length === 0) return { errorKey: "cidrsRequired", value: null };

      return {
        errorKey: null,
        value: { source_ip: { cidrs } },
      };
    }
    case "host": {
      const result = buildValuePredicate(values);
      if (!result.value) return result;
      return {
        errorKey: null,
        value: { host: result.value },
      };
    }
    case "header":
    case "query":
    case "cookie": {
      const name = values.name.trim();
      if (!name) return { errorKey: "matcherNameRequired", value: null };

      const result = buildValuePredicate(values);
      if (!result.value || !isRecord(result.value)) return result;

      return {
        errorKey: null,
        value: {
          [values.kind]: {
            name,
            ...result.value,
          },
        },
      };
    }
  }
}
