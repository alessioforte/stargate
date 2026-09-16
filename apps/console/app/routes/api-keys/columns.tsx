import { Badge } from "@mantine/core";
import { type ColumnDef } from "@tanstack/react-table";
import { getTranslation } from "@/i18n";
import { type ApiKey } from "@/services/types";
import { createDetailsColumn } from "@/components";
import { formatDate } from "@/lib/format-date";

export const columns: ColumnDef<ApiKey>[] = [
  {
    accessorKey: "id",
  },
  {
    accessorKey: "label",
    header: () => getTranslation("label").toLowerCase(),
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
  createDetailsColumn<ApiKey>(),
];
