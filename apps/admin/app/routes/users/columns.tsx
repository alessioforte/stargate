import { type ColumnDef } from "@tanstack/react-table";
import { type User } from "@/services/types";
import { getTranslation } from "@/i18n";
import { createDetailsColumn } from "@/components";
import { formatDate } from "@/lib/format-date";

export const columns: ColumnDef<User>[] = [
  {
    accessorKey: "id",
  },
  {
    accessorKey: "email",
  },
  {
    accessorKey: "nickname",
  },
  {
    accessorKey: "givenName",
    header: () => getTranslation("givenName").toLowerCase(),
  },
  {
    accessorKey: "familyName",
    header: () => getTranslation("familyName").toLowerCase(),
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
  createDetailsColumn<User>(),
];
