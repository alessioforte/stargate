export const limitStrategyOptions = [
  { value: "gcra", label: "GCRA" },
  { value: "token_bucket", label: "Token bucket" },
  { value: "quota_tracker", label: "Quota tracker" },
] as const;

export const limitPeriodOptions = [
  { value: "second", label: "Second" },
  { value: "minute", label: "Minute" },
  { value: "hour", label: "Hour" },
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
  { value: "month", label: "Month" },
  { value: "year", label: "Year" },
] as const;

export const loadBalancerStrategyOptions = [
  { value: "round_robin", label: "Round robin" },
  { value: "random", label: "Random" },
  { value: "ip_hash", label: "IP hash" },
] as const;

export const upstreamProtocolOptions = [
  { value: "http1", label: "HTTP/1" },
  { value: "http2", label: "HTTP/2" },
] as const;

export const serviceKindOptions = [
  { value: "load_balancer", label: "Load balancer" },
  { value: "weighted", label: "Weighted" },
  { value: "mirror", label: "Mirror" },
  { value: "failover", label: "Failover" },
  { value: "direct_response", label: "Direct response" },
] as const;

export const responseBodyKindOptions = [
  { value: "none", label: "None" },
  { value: "text", label: "Text" },
  { value: "json", label: "JSON" },
] as const;

export const middlewareKindOptions = [
  { value: "strip_prefix", label: "Strip prefix" },
  { value: "add_prefix", label: "Add prefix" },
  { value: "replace_path_regex", label: "Replace path regex" },
  { value: "preserve_host", label: "Preserve host" },
  { value: "request_headers", label: "Request headers" },
  { value: "response_headers", label: "Response headers" },
] as const;

export const policyKindOptions = [
  { value: "auth", label: "Auth" },
  { value: "access_control", label: "Access control" },
  { value: "rate_limit", label: "Rate limit" },
  { value: "quota", label: "Quota" },
] as const;

export const authStrategyOptions = [
  { value: "jwt", label: "JWT" },
  { value: "api_key", label: "API key" },
] as const;

export const envProfileOptions = [
  { value: "none", label: "None" },
  { value: "basic", label: "Basic" },
  { value: "geo", label: "Geo" },
] as const;

export const policyScopeOptions = [
  { value: "subject", label: "Subject" },
  { value: "org", label: "Org" },
] as const;

export const onMissingOptions = [
  { value: "skip", label: "Skip" },
  { value: "ip_fallback", label: "IP fallback" },
  { value: "deny", label: "Deny" },
] as const;

export const matchKindOptions = [
  { value: "path", label: "Path" },
  { value: "method", label: "Method" },
  { value: "host", label: "Host" },
  { value: "header", label: "Header" },
  { value: "query", label: "Query" },
  { value: "cookie", label: "Cookie" },
  { value: "source_ip", label: "Source IP" },
  { value: "all", label: "All" },
  { value: "any", label: "Any" },
  { value: "not", label: "Not" },
] as const;

export const pathOperatorOptions = [
  { value: "exact", label: "Exact" },
  { value: "prefix", label: "Prefix" },
  { value: "template", label: "Template" },
  { value: "regex", label: "Regex" },
] as const;

export const valueOperatorOptions = [
  { value: "eq", label: "Equals" },
  { value: "prefix", label: "Prefix" },
  { value: "suffix", label: "Suffix" },
  { value: "contains", label: "Contains" },
  { value: "regex", label: "Regex" },
  { value: "present", label: "Present" },
  { value: "one_of", label: "One of" },
] as const;

export type LimitStrategy = (typeof limitStrategyOptions)[number]["value"];
export type LimitPeriod = (typeof limitPeriodOptions)[number]["value"];
export type LoadBalancerStrategy =
  (typeof loadBalancerStrategyOptions)[number]["value"];
export type UpstreamProtocol =
  (typeof upstreamProtocolOptions)[number]["value"];
export type ServiceKind = (typeof serviceKindOptions)[number]["value"];
export type ResponseBodyKind =
  (typeof responseBodyKindOptions)[number]["value"];
export type MiddlewareKind = (typeof middlewareKindOptions)[number]["value"];
export type PolicyKind = (typeof policyKindOptions)[number]["value"];
export type AuthStrategy = (typeof authStrategyOptions)[number]["value"];
export type EnvProfile = (typeof envProfileOptions)[number]["value"];
export type PolicyScope = (typeof policyScopeOptions)[number]["value"];
export type OnMissing = (typeof onMissingOptions)[number]["value"];
export type MatchKind = (typeof matchKindOptions)[number]["value"];
export type PathOperator = (typeof pathOperatorOptions)[number]["value"];
export type ValueOperator = (typeof valueOperatorOptions)[number]["value"];

export function isLimitStrategy(value: unknown): value is LimitStrategy {
  return limitStrategyOptions.some((option) => option.value === value);
}

export function isLimitPeriod(value: unknown): value is LimitPeriod {
  return limitPeriodOptions.some((option) => option.value === value);
}

export function isLoadBalancerStrategy(
  value: unknown,
): value is LoadBalancerStrategy {
  return loadBalancerStrategyOptions.some((option) => option.value === value);
}

export function isUpstreamProtocol(value: unknown): value is UpstreamProtocol {
  return upstreamProtocolOptions.some((option) => option.value === value);
}

export function isServiceKind(value: unknown): value is ServiceKind {
  return serviceKindOptions.some((option) => option.value === value);
}

export function isMiddlewareKind(value: unknown): value is MiddlewareKind {
  return middlewareKindOptions.some((option) => option.value === value);
}

export function isPolicyKind(value: unknown): value is PolicyKind {
  return policyKindOptions.some((option) => option.value === value);
}

export function isAuthStrategy(value: unknown): value is AuthStrategy {
  return authStrategyOptions.some((option) => option.value === value);
}

export function isEnvProfile(value: unknown): value is EnvProfile {
  return envProfileOptions.some((option) => option.value === value);
}

export function isPolicyScope(value: unknown): value is PolicyScope {
  return policyScopeOptions.some((option) => option.value === value);
}

export function isOnMissing(value: unknown): value is OnMissing {
  return onMissingOptions.some((option) => option.value === value);
}

export function isMatchKind(value: unknown): value is MatchKind {
  return matchKindOptions.some((option) => option.value === value);
}

export function isPathOperator(value: unknown): value is PathOperator {
  return pathOperatorOptions.some((option) => option.value === value);
}

export function isValueOperator(value: unknown): value is ValueOperator {
  return valueOperatorOptions.some((option) => option.value === value);
}
