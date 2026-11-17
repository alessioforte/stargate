import { ActionIcon, Center } from "@mantine/core";
import type { ColumnDef } from "@tanstack/react-table";
import { LuChevronRight } from "react-icons/lu";

export interface DetailsColumnMeta<T> {
  open: () => void;
  setSelectedItem: (item: T | null) => void;
}

export function createDetailsColumn<T>(): ColumnDef<T> {
  return {
    id: "details",
    header: () => <></>,
    size: 40,
    cell: (props) => {
      const { open, setSelectedItem } = props.table.options
        .meta as DetailsColumnMeta<T>;

      return (
        <Center>
          <ActionIcon
            color="gray"
            onClick={() => {
              setSelectedItem(props.row.original);
              open();
            }}
          >
            <LuChevronRight size={16} />
          </ActionIcon>
        </Center>
      );
    },
  };
}
