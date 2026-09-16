import type { ApiMessageCatalog, ApiMessageParams } from "@/services/types";

type MessageGroup = "errors" | "messages";

export interface LocalizableApiMessage {
  code?: string;
  message?: string;
  params?: ApiMessageParams;
}

const catalogs = new Map<string, ApiMessageCatalog>();

function localeKey(locale: string) {
  return locale.toLowerCase().split("-")[0];
}

function interpolate(template: string, params?: ApiMessageParams) {
  if (!params) return template;

  return Object.entries(params).reduce(
    (message, [key, value]) =>
      message.replaceAll(`{{${key}}}`, value === null ? "" : String(value)),
    template,
  );
}

export function registerApiMessageCatalog(catalog: ApiMessageCatalog) {
  catalogs.set(localeKey(catalog.locale), catalog);
}

export function resolveApiMessage(
  locale: string,
  group: MessageGroup,
  response: LocalizableApiMessage,
  fallback: string,
) {
  const template = response.code
    ? catalogs.get(localeKey(locale))?.messages[group][response.code]
    : undefined;

  return interpolate(template ?? response.message ?? fallback, response.params);
}
