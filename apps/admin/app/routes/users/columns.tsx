import { Center, ActionIcon } from "@mantine/core";
import { type ColumnDef } from "@tanstack/react-table";
import { FaAngleRight } from "react-icons/fa";
import { type User } from "@/services/types";
import { getTranslation } from "@/i18n";

interface Meta {
  open: () => void;
  close: () => void;
  setSelectedUser: (user: User | null) => void;
}

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
    accessorKey: "drawer",
    header: () => <></>,
    size: 40,
    cell: (props) => {
      const { open, setSelectedUser } = props.table.options.meta as Meta;
      return (
        <Center>
          <ActionIcon
            variant="transparent"
            onClick={() => {
              setSelectedUser(props.row.original);
              open();
            }}
          >
            <FaAngleRight />
          </ActionIcon>
        </Center>
      );
    },
  },
];
