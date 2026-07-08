import { type ColumnDef } from "@tanstack/react-table";
import { Tooltip } from "@mantine/core";
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
    cell: ({row, table }) => {
      const {superAdminUsers} = table.options.meta as DetailsColumnMeta<User>;
      const isSuperAdmin = superAdminUsers.some((user) => user.id === row.original.id);
      console.log(isSuperAdmin, row.original.id, superAdminUsers);
      return isSuperAdmin ? (
        <Tooltip
          label={getTranslation("adminPermissions.superAdmin")}
          position="right"
        >
          <BsFillShieldFill color="var(--mantine-primary-color-6)" />
        </Tooltip>
      ) : null;
    }
  },
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
