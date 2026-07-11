import { useCallback, useState } from "react";
import {
  Box,
  Combobox,
  Loader,
  ScrollArea,
  TextInput,
  useCombobox,
} from "@mantine/core";
import { useTranslations } from "@/i18n";
import useDebounce from "@/hooks/use-debounce";

export interface AsyncSearchSelectOption {
  value: string;
  label: string;
}

interface AsyncSearchSelectProps {
  label: string;
  placeholder: string;
  fetchFn: (
    query: string,
  ) => Promise<{ data: AsyncSearchSelectOption[] } | AsyncSearchSelectOption[]>;
  value: string | null;
  onChange: (value: string | null) => void;
  error?: React.ReactNode;
  clearable?: boolean;
}

export default function AsyncSearchSelect({
  label,
  placeholder,
  fetchFn,
  value,
  onChange,
  error,
  clearable = true,
}: AsyncSearchSelectProps) {
  const t = useTranslations();
  const [search, setSearch] = useState("");
  const [options, setOptions] = useState<AsyncSearchSelectOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedLabel, setSelectedLabel] = useState("");

  const combobox = useCombobox({
    onDropdownClose: () => {
      if (value) {
        setSearch(selectedLabel);
      } else {
        setSearch("");
        setOptions([]);
      }
    },
  });

  const doFetch = useCallback(
    async (query: string) => {
      if (!query.trim()) return;

      setLoading(true);
      try {
        const result = await fetchFn(query);
        const items = Array.isArray(result)
          ? result
          : ((result as any).data ?? []);
        setOptions(items);
      } catch {
        // request failed — ignore
      } finally {
        setLoading(false);
      }
    },
    [fetchFn],
  );

  const { debounce: debouncedFetch } = useDebounce(doFetch, 900);

  const handleSearchChange = (value: string) => {
    setSearch(value);
    combobox.openDropdown();

    if (!value.trim()) {
      setOptions([]);
      setLoading(false);
    } else {
      debouncedFetch(value);
    }
  };

  const handleOpen = () => {
    combobox.openDropdown();
    if (!search.trim()) {
      setOptions([]);
    }
  };

  const handleOptionSubmit = (val: string) => {
    const option = options.find((o) => o.value === val);
    if (option) {
      setSelectedLabel(option.label);
      setSearch(option.label);
    }
    onChange(val);
    combobox.closeDropdown();
  };

  const handleClear = () => {
    setSearch("");
    setSelectedLabel("");
    setOptions([]);
    onChange(null);
    combobox.openDropdown();
  };

  return (
    <Combobox
      store={combobox}
      onOptionSubmit={handleOptionSubmit}
      floatingHeight="viewport"
    >
      <Combobox.Target>
        <TextInput
          variant="filled"
          label={label}
          placeholder={placeholder}
          value={search}
          onChange={(e) => handleSearchChange(e.target.value)}
          onClick={handleOpen}
          onFocus={handleOpen}
          error={error}
          rightSection={
            clearable && value ? (
              <Combobox.ClearButton onClear={handleClear} />
            ) : undefined
          }
        />
      </Combobox.Target>

      <Combobox.Dropdown>
        <Combobox.Options>
          <ScrollArea.Autosize
            type="scroll"
            mah="var(--combobox-floating-options-max-height)"
          >
            {loading ? (
              <Box p="sm" style={{ textAlign: "center" }}>
                <Loader size="sm" />
              </Box>
            ) : options.length === 0 ? (
              <Combobox.Empty>{t("nothingFound")}</Combobox.Empty>
            ) : (
              options.map((option) => (
                <Combobox.Option key={option.value} value={option.value}>
                  {option.label}
                </Combobox.Option>
              ))
            )}
          </ScrollArea.Autosize>
        </Combobox.Options>
      </Combobox.Dropdown>
    </Combobox>
  );
}
