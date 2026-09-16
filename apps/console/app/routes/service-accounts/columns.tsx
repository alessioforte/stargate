import { type ColumnDef } from "@tanstack/react-table";
import { getTranslation } from "@/i18n";
import { type ServiceAccount } from "@/services/types";
import { createDetailsColumn } from "@/components";
import { formatDate } from "@/lib/format-date";

export const columns: ColumnDef<ServiceAccount>[] = [
  {
    accessorKey: "id",
  },
  {
    accessorKey: "name",
    header: () => getTranslation("name").toLowerCase(),
  },
  {
    accessorKey: "description",
    header: () => getTranslation("description").toLowerCase(),
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
  createDetailsColumn<ServiceAccount>(),
];
