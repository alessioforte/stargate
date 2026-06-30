import type { ColumnDef } from "@tanstack/react-table";
import { createDetailsColumn } from "@/components";
import { getTranslation } from "@/i18n";
import type { GatewayObjectRow } from "./gateway-table-utils";

export const columns: ColumnDef<GatewayObjectRow>[] = [
  {
    accessorKey: "name",
    header: () => getTranslation("name").toLowerCase(),
  },
  {
    accessorKey: "type",
    header: () => getTranslation("kind").toLowerCase(),
  },
  {
    accessorKey: "summary",
    header: () => getTranslation("summary").toLowerCase(),
  },
  createDetailsColumn<GatewayObjectRow>(),
];
