import { format, formatDistanceToNow } from "date-fns";
import type { Locale } from "date-fns";
import { enUS, it } from "date-fns/locale";

const locales: Record<string, Locale> = {
  en: enUS,
  enUS,
  it,
};

const localeOf = (lang?: string) =>
  locales[lang === "en-US" ? "enUS" : (lang ?? "it")] ?? enUS;

/** How long ago `date` was, such as "2 minutes ago". */
export const formatAge = (date: Date | string | number, lang?: string) =>
  formatDistanceToNow(new Date(date), {
    addSuffix: true,
    locale: localeOf(lang),
  });

export const formatDate = (
  date: Date | string | number,
  mask?: string | null,
  lang?: string,
) => {
  if (!date) return "";
  const d = new Date(date);
  return format(d, mask ?? "dd MMM yyyy HH:mm", { locale: localeOf(lang) });
};
