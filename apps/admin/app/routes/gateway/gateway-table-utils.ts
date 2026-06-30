import type { JsonValue } from "@/services/types";
import type { GatewaySection, JsonRecord } from "./config-utils";
import { getPathRecord, isRecord } from "./config-utils";

export interface GatewayObjectRow {
  name: string;
  section: GatewaySection;
  summary: string;
  type: string;
  value: JsonValue;
}

function stringValue(value: JsonValue | undefined, fallback = "") {
  return typeof value === "string" ? value : fallback;
}

function numberValue(value: JsonValue | undefined) {
  return typeof value === "number" ? value : null;
}

function arrayCount(value: JsonValue | undefined) {
  return Array.isArray(value) ? value.length : 0;
}

function recordValue(value: JsonValue | undefined): JsonRecord {
  return isRecord(value) ? value : {};
}

function matchType(value: JsonValue | undefined) {
  if (!isRecord(value)) return "";
  return Object.keys(value)[0] ?? "";
}

function firstPathMatch(value: JsonValue | undefined) {
  if (!isRecord(value)) return "";
  const path = recordValue(value.path);
  const operator = Object.keys(path)[0];
  if (!operator) return "";
  const raw = path[operator];
  return typeof raw === "string" ? raw : "";
}

function limitSummary(value: JsonValue) {
  const params = isRecord(value) ? recordValue(value.params) : {};
  const strategy = isRecord(value) ? stringValue(value.strategy) : "";

  if (strategy === "gcra") {
    return [
      `burst ${numberValue(params.max_burst) ?? "-"}`,
      `replenish ${stringValue(params.replenish_1_per, "-")}`,
    ].join(", ");
  }

  if (strategy === "token_bucket") {
    return [
      `capacity ${numberValue(params.capacity) ?? "-"}`,
      `refill ${numberValue(params.refill_rate) ?? "-"}`,
    ].join(", ");
  }

  if (strategy === "quota_tracker") {
    return [
      `limit ${numberValue(params.limit) ?? "-"}`,
      `period ${stringValue(params.period, "-")}`,
    ].join(", ");
  }

  return "";
}

function upstreamSummary(value: JsonValue) {
  const record = recordValue(value);
  const loadBalancer = recordValue(record.load_balancer);
  const targetCount = arrayCount(record.targets);
  const transport = isRecord(record.transport) ? "transport" : "";
  const probe = isRecord(loadBalancer.liveness_probe) ? "probe" : "";
  const breaker = isRecord(loadBalancer.circuit_breaker) ? "breaker" : "";

  return [
    `${targetCount} ${targetCount === 1 ? "target" : "targets"}`,
    probe,
    breaker,
    transport,
  ]
    .filter(Boolean)
    .join(", ");
}

function serviceSummary(value: JsonValue) {
  const record = recordValue(value);
  const kind = stringValue(record.kind);

  if (kind === "load_balancer") return stringValue(record.upstream);
  if (kind === "weighted") return `${arrayCount(record.services)} services`;
  if (kind === "mirror") return `${arrayCount(record.mirrors)} mirrors`;
  if (kind === "failover") return `${arrayCount(record.failovers)} failovers`;
  if (kind === "direct_response")
    return `status ${numberValue(record.status) ?? "-"}`;

  return "";
}

function middlewareSummary(value: JsonValue) {
  const record = recordValue(value);
  const kind = stringValue(record.kind);

  if (kind === "strip_prefix") return `${arrayCount(record.prefixes)} prefixes`;
  if (kind === "add_prefix") return stringValue(record.prefix);
  if (kind === "replace_path_regex") return stringValue(record.pattern);
  if (kind === "preserve_host") return "";
  if (kind === "request_headers" || kind === "response_headers") {
    return [
      `${arrayCount(record.add)} add`,
      `${arrayCount(record.set)} set`,
      `${arrayCount(record.remove)} remove`,
    ].join(", ");
  }

  return "";
}

function policySummary(value: JsonValue) {
  const record = recordValue(value);
  const kind = stringValue(record.kind);

  if (kind === "auth") return `${arrayCount(record.strategies)} strategies`;
  if (kind === "access_control") return stringValue(record.resource);
  if (kind === "rate_limit") return stringValue(record.limit);
  if (kind === "quota") {
    return [
      stringValue(record.limit),
      `cost ${numberValue(record.cost) ?? "-"}`,
    ]
      .filter(Boolean)
      .join(", ");
  }

  return "";
}

function routerSummary(value: JsonValue) {
  const record = recordValue(value);
  const path = firstPathMatch(record.match);
  const service = stringValue(record.service);
  const priority = numberValue(record.priority);

  return [
    priority === null ? "" : `priority ${priority}`,
    path,
    service ? `-> ${service}` : "",
  ]
    .filter(Boolean)
    .join(", ");
}

export function getGatewayObjectType(
  section: GatewaySection,
  value: JsonValue,
) {
  const record = recordValue(value);

  if (section.key === "limits") return stringValue(record.strategy);
  if (section.key === "upstreams") {
    return stringValue(recordValue(record.load_balancer).strategy);
  }
  if (
    section.key === "services" ||
    section.key === "middlewares" ||
    section.key === "policies"
  ) {
    return stringValue(record.kind);
  }
  if (section.key === "routers") return matchType(record.match);

  return "";
}

export function getGatewayObjectSummary(
  section: GatewaySection,
  value: JsonValue,
) {
  if (section.key === "limits") return limitSummary(value);
  if (section.key === "upstreams") return upstreamSummary(value);
  if (section.key === "services") return serviceSummary(value);
  if (section.key === "middlewares") return middlewareSummary(value);
  if (section.key === "policies") return policySummary(value);
  if (section.key === "routers") return routerSummary(value);

  return "";
}

export function getGatewayObjectRows(
  config: JsonValue | undefined | null,
  section: GatewaySection,
): GatewayObjectRow[] {
  return Object.entries(getPathRecord(config, section.path))
    .sort(([nameA], [nameB]) => nameA.localeCompare(nameB))
    .map(([name, value]) => ({
      name,
      section,
      summary: getGatewayObjectSummary(section, value),
      type: getGatewayObjectType(section, value),
      value,
    }));
}
