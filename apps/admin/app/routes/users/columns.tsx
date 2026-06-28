import { type ColumnDef } from "@tanstack/react-table";
import { type User } from "@/services/types";
import { getTranslation } from "@/i18n";
import { createDetailsColumn } from "@/components";

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
  createDetailsColumn<User>(),
];
