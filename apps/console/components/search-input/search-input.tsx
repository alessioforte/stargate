import { TextInput, type TextInputProps } from "@mantine/core";
import { LuSearch } from "react-icons/lu";
import useDebounce from "@/hooks/use-debounce";

interface SearchInputProps extends TextInputProps {
  onSearch?: (query: string) => void;
}

const SearchInput = (props: SearchInputProps) => {
  const { onSearch, ...rest } = props;
  const { debounce: debouncedOnSearch } = useDebounce(
    onSearch ?? (() => {}),
    900,
  );

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (rest.onChange) rest.onChange(e);
    debouncedOnSearch(e.target.value);
  };

  return (
    <TextInput
      size="sm"
      w={280}
      maw="100%"
      type="search"
      leftSection={<LuSearch size={15} />}
      {...rest}
      onChange={handleChange}
    />
  );
};

export default SearchInput;
