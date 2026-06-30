import type { JsonValue } from "@/services/types";

export type JsonRecord = Record<string, JsonValue>;

export interface GatewaySection {
  key: string;
  labelKey: string;
  path: string[];
}

export const gatewaySections: GatewaySection[] = [
  { key: "limits", labelKey: "limits", path: ["limits"] },
  { key: "upstreams", labelKey: "upstreams", path: ["http", "upstreams"] },
  { key: "services", labelKey: "services", path: ["http", "services"] },
  {
    key: "middlewares",
    labelKey: "middlewares",
    path: ["http", "middlewares"],
  },
  { key: "policies", labelKey: "policies", path: ["http", "policies"] },
  { key: "routers", labelKey: "routers", path: ["http", "routers"] },
];

export function isRecord(
  value: JsonValue | undefined | null,
): value is JsonRecord {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function getPathRecord(
  config: JsonValue | undefined | null,
  path: string[],
): JsonRecord {
  let current: JsonValue | undefined | null = config;

  for (const segment of path) {
    if (!isRecord(current)) return {};
    current = current[segment];
  }

  return isRecord(current) ? current : {};
}

export function getSchema(config: JsonValue | undefined | null) {
  if (!isRecord(config)) return "";
  const schema = config.schema;
  return typeof schema === "string" ? schema : "";
}

export function stringifyJson(value: JsonValue | undefined | null) {
  return JSON.stringify(value ?? {}, null, 2);
}
