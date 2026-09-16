import { type ColumnDef } from "@tanstack/react-table";
import { type Organization } from "@/services/types";
import { getTranslation } from "@/i18n";
import { createDetailsColumn } from "@/components";
import { formatDate } from "@/lib/format-date";

export const columns: ColumnDef<Organization>[] = [
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
  createDetailsColumn<Organization>(),
];
