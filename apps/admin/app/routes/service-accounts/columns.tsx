import { type ColumnDef } from "@tanstack/react-table";
import { getTranslation } from "@/i18n";
import { type ServiceAccount } from "@/services/types";
import { createDetailsColumn } from "@/components";

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
    accessorKey: "orgId",
    header: () => getTranslation("orgId").toLowerCase(),
  },
  createDetailsColumn<ServiceAccount>(),
];
