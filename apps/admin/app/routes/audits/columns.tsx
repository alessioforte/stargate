import { Badge, Code, Group } from "@mantine/core";
import { type ColumnDef } from "@tanstack/react-table";
import { type OutboxEvent } from "@/services/types";
import { getTranslation } from "@/i18n";
import { createDetailsColumn } from "@/components";
import { formatDate } from "@/lib/format-date";

export const columns: ColumnDef<OutboxEvent>[] = [
  {
    accessorKey: "seq",
    header: () => getTranslation("seq").toLowerCase(),
    size: 50,
  },
  {
    accessorKey: "eventId",
    header: () => getTranslation("eventId").toLowerCase(),
    cell: ({ row }) => (
      <Code style={{ fontSize: 12 }}>{row.original.eventId}</Code>
    ),
  },
  {
    accessorKey: "operationId",
    header: () => getTranslation("operationId").toLowerCase(),
    cell: ({ row }) =>
      row.original.operationId ? (
        <Code style={{ fontSize: 12 }}>{row.original.operationId}</Code>
      ) : (
        <span style={{ color: "var(--mantine-color-dimmed)" }}>—</span>
      ),
  },
  {
    accessorKey: "pairRole",
    header: () => getTranslation("pairRole").toLowerCase(),
    cell: ({ row }) =>
      row.original.pairRole ? (
        <Badge variant="light" color="violet">
          {row.original.pairRole}
        </Badge>
      ) : (
        <span style={{ color: "var(--mantine-color-dimmed)" }}>—</span>
      ),
  },
  {
    accessorKey: "payload.action",
    header: () => getTranslation("action").toLowerCase(),
    size: 200,
    cell: ({ row }) => {
      const payload = row.original.payload as { action?: string };
      const [resource, action] = payload?.action?.split(".") ?? [];
      return (
        <Group gap={5} wrap="nowrap">
          {resource && <Badge variant="transparent">{resource}</Badge>}
          {action && <Badge color="pink">{action}</Badge>}
        </Group>
      );
    },
  },
  {
    accessorKey: "status",
    header: () => getTranslation("status").toLowerCase(),
    size: 90,
    cell: ({ row }) => (
      <Badge color={row.original.status === "published" ? "teal" : "yellow"}>
        {row.original.status}
      </Badge>
    ),
  },
  {
    accessorKey: "createdAt",
    header: () => getTranslation("createdAt").toLowerCase(),
    cell: (props) => formatDate(props.row.original.createdAt),
  },
  {
    accessorKey: "publishedAt",
    header: () => getTranslation("publishedAt").toLowerCase(),
    cell: (props) =>
      props.row.original.publishedAt ? (
        formatDate(props.row.original.publishedAt)
      ) : (
        <span style={{ color: "var(--mantine-color-dimmed)" }}>—</span>
      ),
  },
  createDetailsColumn<OutboxEvent>(),
];
