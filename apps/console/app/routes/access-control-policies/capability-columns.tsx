import { Badge, Group, Text } from "@mantine/core";
import type { ColumnDef } from "@tanstack/react-table";
import { getTranslation } from "@/i18n";
import type { AccessControlCapability } from "@/services/types";

export const accessControlCapabilityColumns: ColumnDef<AccessControlCapability>[] =
  [
    {
      size: 80,
      accessorKey: "resource",
      header: () => getTranslation("resource").toLowerCase(),
    },
    {
      size: 80,
      accessorKey: "unscopedAllowed",
      header: () => getTranslation("unscopedAccess").toLowerCase(),
      cell: ({ row }) => (
        <Badge color={row.original.unscopedAllowed ? "teal" : "gray"}>
          {row.original.unscopedAllowed
            ? getTranslation("allowed")
            : getTranslation("denied")}
        </Badge>
      ),
    },
    {
      accessorKey: "actions",
      header: () => getTranslation("actions").toLowerCase(),
      cell: ({ row }) =>
        row.original.actions.length > 0 ? (
          <Group gap={4}>
            {row.original.actions.map((action) => (
              <Badge key={action} color="indigo" variant="light">
                {action}
              </Badge>
            ))}
          </Group>
        ) : (
          <Text size="sm" c="dimmed">
            -
          </Text>
        ),
    },
  ];
