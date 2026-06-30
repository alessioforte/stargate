import {
  Badge,
  ActionIcon,
  Tooltip,
  Group,
  Stack,
  Tabs,
  Title,
} from "@mantine/core";
import { FaPlus } from "react-icons/fa6";
import { Table } from "@/components";
import { useTranslations } from "@/i18n";
import { columns } from "./columns";
import { gatewaySections } from "./config-utils";
import type { GatewayObjectRow } from "./gateway-table-utils";

interface Props {
  loading: boolean;
  rows: GatewayObjectRow[];
  sectionCounts: Record<string, number>;
  selectedSectionKey: string;
  onCreate: () => void;
  onOpen: () => void;
  onSectionChange: (value: string | null) => void;
  onSelect: (row: GatewayObjectRow | null) => void;
}

const GatewaySectionTable: React.FC<Props> = ({
  loading,
  rows,
  sectionCounts,
  selectedSectionKey,
  onCreate,
  onOpen,
  onSectionChange,
  onSelect,
}) => {
  const t = useTranslations();

  return (
    <Tabs value={selectedSectionKey} onChange={onSectionChange}>
      <Tabs.List>
        {gatewaySections.map((section) => (
          <Tabs.Tab key={section.key} value={section.key}>
            <Group gap={6}>
              {t(section.labelKey)}
              <Badge size="sm" variant="light">
                {sectionCounts[section.key] ?? 0}
              </Badge>
            </Group>
          </Tabs.Tab>
        ))}
      </Tabs.List>

      <Stack mt="sm" gap="sm">
        <Group p="xs" justify="space-between">
          <Title order={4}>{t(selectedSectionKey)}</Title>
          <Tooltip label={t("newObject")} position="left" offset={10}>
            <ActionIcon onClick={onCreate}>
              <FaPlus />
            </ActionIcon>
          </Tooltip>
        </Group>
        <Table
          columns={columns}
          data={rows}
          empty={rows.length === 0}
          loading={loading}
          meta={{
            open: onOpen,
            setSelectedItem: onSelect,
          }}
        />
      </Stack>
    </Tabs>
  );
};

export default GatewaySectionTable;
