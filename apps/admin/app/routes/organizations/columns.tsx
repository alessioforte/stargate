import { type ColumnDef } from "@tanstack/react-table";
import { type Organization } from "@/services/types";
import { getTranslation } from "@/i18n";
import { createDetailsColumn } from "@/components";

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
  createDetailsColumn<Organization>(),
];
