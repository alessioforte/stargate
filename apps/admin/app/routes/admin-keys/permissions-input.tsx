import { Checkbox, SimpleGrid, Stack, Text } from "@mantine/core";
import type { ReactNode } from "react";
import { useTranslations } from "@/i18n";
import type { AdminKeyPermission } from "@/services/types";
import {
  adminKeyPermissionGroups,
  getAdminKeyPermissionActionLabel,
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
      <SimpleGrid mt="xs" cols={{ base: 1, sm: 2 }} spacing="md">
        {adminKeyPermissionGroups.map((group) => (
          <Stack key={group.labelKey} gap={4}>
            <Text size="sm" fw={600}>
              {t(group.labelKey)}
            </Text>
            {group.permissions.map((permission) => (
              <Checkbox
                key={permission}
                disabled={disabled}
                value={permission}
                label={getAdminKeyPermissionActionLabel(permission)}
              />
            ))}
          </Stack>
        ))}
      </SimpleGrid>
    </Checkbox.Group>
  );
};

export default AdminKeyPermissionsInput;
