import { Badge, Text } from "@mantine/core";
import type { ColumnDef } from "@tanstack/react-table";
import { getTranslation } from "@/i18n";
import type { AccessControlRule } from "@/services/types";

export const accessControlRuleColumns: ColumnDef<AccessControlRule>[] = [
  {
    size: 80,
    accessorKey: "effect",
    header: () => getTranslation("effect").toLowerCase(),
    cell: ({ row }) => (
      <Badge color={row.original.effect === "allow" ? "teal" : "red"}>
        {row.original.effect}
      </Badge>
    ),
  },
  {
    size: 80,
    accessorKey: "subject",
    header: () => getTranslation("subject").toLowerCase(),
  },
  {
    size: 120,
    accessorKey: "resource",
    header: () => getTranslation("resource").toLowerCase(),
    cell: ({ row }) => (
      <Text size="sm">
        {row.original.resource}
        {row.original.action ? `:${row.original.action}` : ""}
      </Text>
    ),
  },
  {
    accessorKey: "condition",
    header: () => getTranslation("condition").toLowerCase(),
    cell: ({ row }) => (
      <Text size="sm" maw={520} lineClamp={2}>
        {row.original.condition ?? "-"}
      </Text>
    ),
  },
];
