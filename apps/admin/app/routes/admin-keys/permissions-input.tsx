import { Checkbox, Stack } from "@mantine/core";
import type { ReactNode } from "react";
import { useTranslations } from "@/i18n";
import type { AdminKeyPermission } from "@/services/types";
import {
  adminKeyPermissionValues,
  permissionTranslationKeys,
} from "./permissions";

interface Props {
  disabled?: boolean;
  error?: ReactNode;
  value: AdminKeyPermission[];
  onChange: (value: AdminKeyPermission[]) => void;
}

const AdminKeyPermissionsInput: React.FC<Props> = ({
  disabled = false,
  error,
  value,
  onChange,
}) => {
  const t = useTranslations();

  return (
    <Checkbox.Group
      label={t("permissions")}
      value={value}
      error={error}
      onChange={(value) => onChange(value as AdminKeyPermission[])}
    >
      <Stack mt="xs" gap="xs">
        {adminKeyPermissionValues.map((permission) => (
          <Checkbox
            key={permission}
            disabled={disabled}
            value={permission}
            label={t(permissionTranslationKeys[permission])}
          />
        ))}
      </Stack>
    </Checkbox.Group>
  );
};

export default AdminKeyPermissionsInput;
