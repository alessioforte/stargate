import {
  ActionIcon,
  Badge,
  Code,
  Group,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import type { ColumnDef } from "@tanstack/react-table";
import { HiOutlineUsers } from "react-icons/hi2";
import { CiLogout } from "react-icons/ci";
import { getTranslation } from "@/i18n";
import { formatDate } from "@/lib/format-date";
import type { AdminSession } from "@/services/types";

interface SessionsTableMeta {
  onRevokeSession: (session: AdminSession) => void;
  onRevokeUserSessions: (session: AdminSession) => void;
}

function emptyValue() {
  return <Text c="dimmed">—</Text>;
}

export const columns: ColumnDef<AdminSession>[] = [
  {
    accessorKey: "current",
    header: () => getTranslation("status").toLowerCase(),
    size: 80,
    cell: ({ row }) =>
      row.original.current ? (
        <Badge color="blue" variant="light">
          {getTranslation("current")}
        </Badge>
      ) : (
        <Badge color="teal" variant="light">
          {getTranslation("active")}
        </Badge>
      ),
  },
  {
    accessorKey: "user",
    header: () => getTranslation("user").toLowerCase(),
    size: 210,
    cell: ({ row }) => (
      <Stack gap={0}>
        <Text size="sm" fw={500} truncate>
          {row.original.user.name ??
            row.original.user.email ??
            row.original.user.id}
        </Text>
        <Text size="xs" c="dimmed" truncate>
          {row.original.user.email ?? row.original.user.id}
        </Text>
      </Stack>
    ),
  },
  {
    accessorKey: "id",
    header: () => getTranslation("sessionId").toLowerCase(),
    size: 210,
    cell: ({ row }) => <Code fz={11}>{row.original.id}</Code>,
  },
  {
    accessorKey: "organizationId",
    header: () => getTranslation("organizationId").toLowerCase(),
    size: 170,
    cell: ({ row }) =>
      row.original.organizationId ? (
        <Code fz={11}>{row.original.organizationId}</Code>
      ) : (
        emptyValue()
      ),
  },
  {
    accessorKey: "clientIds",
    header: () => getTranslation("linkedClients").toLowerCase(),
    size: 180,
    cell: ({ row }) =>
      row.original.clientIds.length > 0 ? (
        <Group gap={4}>
          {row.original.clientIds.map((clientId) => (
            <Badge key={clientId} variant="light" color="violet">
              {clientId}
            </Badge>
          ))}
        </Group>
      ) : (
        emptyValue()
      ),
  },
  {
    accessorKey: "authenticatedAt",
    header: () => getTranslation("authenticatedAt").toLowerCase(),
    size: 155,
    cell: ({ row }) => formatDate(row.original.authenticatedAt),
  },
  {
    accessorKey: "lastSeenAt",
    header: () => getTranslation("lastSeenAt").toLowerCase(),
    size: 155,
    cell: ({ row }) => formatDate(row.original.lastSeenAt),
  },
  {
    accessorKey: "expiresAt",
    header: () => getTranslation("expiresAt").toLowerCase(),
    size: 155,
    cell: ({ row }) => formatDate(row.original.expiresAt),
  },
  {
    id: "actions",
    header: () => <></>,
    size: 74,
    cell: ({ row, table }) => {
      const meta = table.options.meta as unknown as SessionsTableMeta;
      return (
        <Group gap={4} justify="flex-end" wrap="nowrap">
          <Tooltip label={getTranslation("revokeSession")}>
            <ActionIcon
              color="red"
              variant="subtle"
              aria-label={getTranslation("revokeSession")}
              onClick={() => meta.onRevokeSession(row.original)}
            >
              <CiLogout size={16} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label={getTranslation("revokeAllUserSessions")}>
            <ActionIcon
              color="red"
              variant="subtle"
              aria-label={getTranslation("revokeAllUserSessions")}
              onClick={() => meta.onRevokeUserSessions(row.original)}
            >
              <HiOutlineUsers size={16} />
            </ActionIcon>
          </Tooltip>
        </Group>
      );
    },
  },
];
