import { Badge, Group } from "@mantine/core";
import { type ColumnDef } from "@tanstack/react-table";
import { type AdminKey } from "@/services/types";
import { getTranslation } from "@/i18n";
import { createDetailsColumn } from "@/components";
import { getAdminKeyPermissionLabel } from "./permissions";
import { formatDate } from "@/lib/format-date";

export const columns: ColumnDef<AdminKey>[] = [
  {
    accessorKey: "id",
  },
  {
    accessorKey: "label",
    header: () => getTranslation("label").toLowerCase(),
  },
  {
    accessorKey: "permissions",
    header: () => getTranslation("permissions").toLowerCase(),
    cell: ({ row }) => (
      <Group gap={4}>
        {row.original.permissions.map((permission) => (
          <Badge key={permission} color="blue">
            {getAdminKeyPermissionLabel(permission)}
          </Badge>
        ))}
      </Group>
    ),
  },
  {
    accessorKey: "createdAt",
    header: () => getTranslation("createdAt").toLowerCase(),
    cell: (props) => formatDate(props.row.original.createdAt),
  },
  {
    accessorKey: "updatedAt",
    header: () => getTranslation("updatedAt").toLowerCase(),
    cell: (props) => formatDate(props.row.original.updatedAt),
  },
  {
    accessorKey: "revoked",
    header: () => getTranslation("status").toLowerCase(),
    cell: ({ row }) => (
      <Badge color={row.original.revoked ? "red" : "teal"}>
        {row.original.revoked
          ? getTranslation("revoked")
          : getTranslation("active")}
      </Badge>
    ),
  },
  createDetailsColumn<AdminKey>(),
];
