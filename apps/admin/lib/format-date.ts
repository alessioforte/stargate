import * as DateFNS from "date-fns";
import * as locals from "date-fns/locale";

export const formatDate = (
  date: Date | string | number,
  mask?: string | null,
  lang?: string,
) => {
  if (!date) return "";
  const d = new Date(date);
  const locale =
    (locals as any)[lang === "en" ? "enUS" : (lang ?? "it")] ||
    (locals as any).enUS;
  return DateFNS.format(d, mask ?? "dd MMM yyyy HH:mm", { locale });
};
