import { useCallback } from "react";
import en from "./locales/en.json";
import it from "./locales/it.json";
import useStore from "@/store";

const messages = {
  en,
  it,
} as const;

type Locale = keyof typeof messages;
type TranslationValues = Record<string, string | number>;

function normalizeLocale(locale: string): Locale {
  return locale === "it" ? "it" : "en";
}

function resolveMessage(source: object, key: string): string | null {
  const value = key.split(".").reduce<unknown>((current, part) => {
    if (!current || typeof current !== "object") return undefined;
    return (current as Record<string, unknown>)[part];
  }, source);

  return typeof value === "string" ? value : null;
}

function interpolate(message: string, values?: TranslationValues) {
  if (!values) return message;

  return Object.entries(values).reduce(
    (text, [key, value]) => text.replaceAll(`{{${key}}}`, String(value)),
    message,
  );
}

export function translate(
  locale: string,
  key: string,
  values?: TranslationValues,
) {
  const normalizedLocale = normalizeLocale(locale);
  const message =
    resolveMessage(messages[normalizedLocale], key) ??
    resolveMessage(messages.en, key) ??
    key;

  return interpolate(message, values);
}

export function useTranslations() {
  const language = useStore((state) => state.language);

  return useCallback(
    (key: string, values?: TranslationValues) =>
      translate(language, key, values),
    [language],
  );
}
