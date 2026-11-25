import { format } from "date-fns";
import type { Locale } from "date-fns";
import { enUS, it } from "date-fns/locale";

const locales: Record<string, Locale> = {
  en: enUS,
  enUS,
  it,
};

const localeOf = (lang?: string) =>
  locales[lang === "en-US" ? "enUS" : (lang ?? "it")] ?? enUS;

export const formatDate = (
  date: Date | string | number,
  mask?: string | null,
  lang?: string,
) => {
  if (!date) return "";
  const d = new Date(date);
  return format(d, mask ?? "dd MMM yyyy HH:mm", { locale: localeOf(lang) });
};
