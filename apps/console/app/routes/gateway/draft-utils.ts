import type { JsonValue } from "@/services/types";
import type { GatewaySection, JsonRecord } from "./config-utils";
import { GATEWAY_SCHEMA, isRecord } from "./config-utils";

export function cloneJson<T extends JsonValue>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

export function createEmptyGatewayConfig(): JsonRecord {
  return {
    schema: GATEWAY_SCHEMA,
    runtime: {},
    limits: {},
    http: {
      upstreams: {},
      services: {},
      middlewares: {},
      policies: {},
      routers: {},
    },
  };
}

export function normalizeGatewayConfig(config: JsonValue | undefined | null) {
  if (!isRecord(config)) return createEmptyGatewayConfig();

  const next = cloneJson(config);
  if (!isRecord(next.http)) {
    next.http = {};
  }

  return next;
}

function ensurePathRecord(config: JsonRecord, path: string[]) {
  let current = config;

  for (const segment of path) {
    if (!isRecord(current[segment])) {
      current[segment] = {};
    }
    current = current[segment] as JsonRecord;
  }

  return current;
}

export function renameSectionObject(
  config: JsonValue | undefined | null,
  section: GatewaySection,
  previousName: string,
  nextName: string,
  value: JsonValue,
) {
  const next = normalizeGatewayConfig(config);
  const sectionRecord = ensurePathRecord(next, section.path);
  if (previousName !== nextName) {
    delete sectionRecord[previousName];
  }
  sectionRecord[nextName] = cloneJson(value);
  return next;
}

export function deleteSectionObject(
  config: JsonValue | undefined | null,
  section: GatewaySection,
  name: string,
) {
  const next = normalizeGatewayConfig(config);
  const sectionRecord = ensurePathRecord(next, section.path);
  delete sectionRecord[name];
  return next;
}

export function nextObjectName(existingNames: string[], baseName: string) {
  if (!existingNames.includes(baseName)) return baseName;

  let index = 2;
  while (existingNames.includes(`${baseName}-${index}`)) {
    index += 1;
  }

  return `${baseName}-${index}`;
}
