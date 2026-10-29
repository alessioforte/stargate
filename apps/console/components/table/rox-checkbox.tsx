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
