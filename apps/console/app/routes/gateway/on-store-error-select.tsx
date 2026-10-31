import { Select } from "@mantine/core";
import { useTranslations } from "@/i18n";

export type OnStoreError = "allow" | "deny";

interface Props {
  value: string;
  onChange: (value: OnStoreError) => void;
  ingress?: boolean;
}

export default function OnStoreErrorSelect({
  value,
  onChange,
  ingress,
}: Props) {
  const t = useTranslations();
  return (
    <Select
      label={t("onStoreError")}
      description={t(
        ingress ? "ingressOnStoreErrorDescription" : "onStoreErrorDescription",
      )}
      data={[
        { value: "allow", label: t("onStoreErrorAllow") },
        { value: "deny", label: t("onStoreErrorDeny") },
      ]}
      value={value}
      allowDeselect={false}
      onChange={(mode) => mode && onChange(mode as OnStoreError)}
    />
  );
}
