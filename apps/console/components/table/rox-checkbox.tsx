import { Checkbox } from "@mantine/core";

function IndeterminateCheckbox({
  indeterminate,
  checked,
  disabled,
  onChange,
}: { indeterminate?: boolean } & React.HTMLProps<HTMLInputElement>) {
  return (
    <Checkbox
      size="xs"
      indeterminate={indeterminate}
      checked={checked}
      disabled={disabled}
      onChange={onChange}
    />
  );
}

export default IndeterminateCheckbox;

export const selectionColumn: any = [
  {
    id: "select",
    header: ({ table }: any) => (
      <IndeterminateCheckbox
        {...{
          checked: table.getIsAllRowsSelected(),
          indeterminate: table.getIsSomeRowsSelected(),
          onChange: table.getToggleAllRowsSelectedHandler(),
        }}
      />
    ),
    cell: ({ row }: any) => (
      <div className="px-1">
        <IndeterminateCheckbox
          {...{
            checked: row.getIsSelected(),
            disabled: !row.getCanSelect(),
            indeterminate: row.getIsSomeSelected(),
            onChange: row.getToggleSelectedHandler(),
          }}
        />
      </div>
    ),
  },
];
