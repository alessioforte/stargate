import { ActionIcon, Group, Text, TextInput, Tooltip } from "@mantine/core";
import { IoCloseSharp } from "react-icons/io5";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";

export interface HeaderValueRow {
  id: string;
  name: string;
  value: string;
}

interface Props {
  addLabelKey?: string;
  emptyLabelKey?: string;
  rows: HeaderValueRow[];
  titleKey: string;
  onAdd: () => void;
  onRemove: (id: string) => void;
  onUpdate: (id: string, patch: Partial<HeaderValueRow>) => void;
}

const HeaderValueRows: React.FC<Props> = ({
  addLabelKey = "addHeader",
  emptyLabelKey = "noHeaders",
  rows,
  titleKey,
  onAdd,
  onRemove,
  onUpdate,
}) => {
  const t = useTranslations();

  return (
    <>
      <Group mt="md" justify="space-between">
        <Text fw={700}>{t(titleKey)}</Text>

        <Tooltip label={t(addLabelKey)} position="left" offset={10}>
          <ActionIcon size="sm" onClick={onAdd}>
            <FaPlus />
          </ActionIcon>
        </Tooltip>
      </Group>
      {rows.length === 0 && (
        <Text size="sm" c="dimmed">
          {t(emptyLabelKey)}
        </Text>
      )}
      {rows.map((header) => (
        <Group key={header.id} align="center" wrap="nowrap">
          <TextInput
            size="xs"
            variant="filled"
            label={t("headerName")}
            value={header.name}
            onChange={(event) =>
              onUpdate(header.id, { name: event.currentTarget.value })
            }
            style={{ flex: 1 }}
          />
          <TextInput
            size="xs"
            variant="filled"
            label={t("headerValue")}
            value={header.value}
            onChange={(event) =>
              onUpdate(header.id, { value: event.currentTarget.value })
            }
            style={{ flex: 1 }}
          />
          <Tooltip label={t("delete")}>
            <ActionIcon
              mt={25}
              size="md"
              color="red"
              type="button"
              variant="transparent"
              onClick={() => onRemove(header.id)}
            >
              <IoCloseSharp />
            </ActionIcon>
          </Tooltip>
        </Group>
      ))}
    </>
  );
};

export default HeaderValueRows;
