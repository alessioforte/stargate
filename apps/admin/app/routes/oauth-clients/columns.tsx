import { Badge, Group } from "@mantine/core";
import { type ColumnDef } from "@tanstack/react-table";
import { getTranslation } from "@/i18n";
import { type OAuthClient } from "@/services/types";
import { createDetailsColumn } from "@/components";
import {
  getOAuthGrantTypeLabel,
  getOAuthTokenEndpointAuthMethodLabel,
} from "./options";

export const columns: ColumnDef<OAuthClient>[] = [
  {
    accessorKey: "clientId",
    header: () => getTranslation("clientId").toLowerCase(),
  },
  {
    accessorKey: "name",
    header: () => getTranslation("name").toLowerCase(),
  },
  {
    accessorKey: "tokenEndpointAuthMethod",
    header: () => getTranslation("tokenEndpointAuthMethod").toLowerCase(),
    cell: ({ row }) =>
      getOAuthTokenEndpointAuthMethodLabel(
        row.original.tokenEndpointAuthMethod,
      ),
  },
  {
    accessorKey: "grantTypes",
    header: () => getTranslation("grantTypes").toLowerCase(),
    cell: ({ row }) => (
      <Group gap={4}>
        {row.original.grantTypes.map((grantType) => (
          <Badge key={grantType} color="cyan">
            {getOAuthGrantTypeLabel(grantType)}
          </Badge>
        ))}
      </Group>
    ),
  },
  {
    accessorKey: "enabled",
    header: () => getTranslation("status").toLowerCase(),
    cell: ({ row }) => (
      <Badge color={row.original.enabled ? "teal" : "red"}>
        {row.original.enabled
          ? getTranslation("enabled")
          : getTranslation("disabled")}
      </Badge>
    ),
  },
  createDetailsColumn<OAuthClient>(),
];
