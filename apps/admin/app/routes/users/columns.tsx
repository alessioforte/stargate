import { type ColumnDef } from "@tanstack/react-table";
import { Tooltip, Text } from "@mantine/core";
import { BsFillShieldFill } from "react-icons/bs";
import { type User } from "@/services/types";
import { getTranslation } from "@/i18n";
import { createDetailsColumn } from "@/components";
import { formatDate } from "@/lib/format-date";

interface DetailsColumnMeta<T> {
  superAdminUsers: T[];
}

export const columns: ColumnDef<User>[] = [
  {
    accessorKey: "isSuperAdmin",
    size: 30,
    header: () => <></>,
    cell: ({ row, table }) => {
      const { superAdminUsers } = table.options.meta as DetailsColumnMeta<User>;
      const isSuperAdmin = superAdminUsers.some(
        (user) => user.id === row.original.id,
      );
      return isSuperAdmin ? (
        <Tooltip
          label={getTranslation("adminPermissions.superAdmin")}
          position="right"
        >
          <BsFillShieldFill color="var(--mantine-primary-color-6)" />
        </Tooltip>
      ) : null;
    },
  },
  {
    accessorKey: "id",
    cell: ({ row }) => (
      <Text size="xs" truncate>
        {row.original.id}
      </Text>
    ),
  },
  {
    accessorKey: "email",
    cell: ({ row }) => (
      <Text size="xs" truncate>
        {row.original.email}
      </Text>
    ),
  },
  {
    accessorKey: "nickname",
    cell: ({ row }) => (
      <Text size="xs" truncate>
        {row.original.nickname}
      </Text>
    ),
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
