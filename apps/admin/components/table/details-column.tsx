import { ActionIcon, Center } from "@mantine/core";
import type { ColumnDef } from "@tanstack/react-table";
import { FaAngleRight } from "react-icons/fa";

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
            variant="transparent"
            onClick={() => {
              setSelectedItem(props.row.original);
              open();
            }}
          >
            <FaAngleRight />
          </ActionIcon>
        </Center>
      );
    },
  };
}
