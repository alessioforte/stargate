import { Badge, Button, Group, Tabs } from "@mantine/core";
import { LuPlus } from "react-icons/lu";
import { Panel, Table } from "@/components";
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
      <Tabs.List mb="md">
        {gatewaySections.map((section) => (
          <Tabs.Tab key={section.key} value={section.key}>
            <Group gap={6}>
              {t(section.labelKey)}
              <Badge size="sm" color="gray">
                {sectionCounts[section.key] ?? 0}
              </Badge>
            </Group>
          </Tabs.Tab>
        ))}
      </Tabs.List>

      <Panel
        title={t(selectedSectionKey)}
        actions={
          <Button
            size="xs"
            variant="default"
            leftSection={<LuPlus size={14} />}
            onClick={onCreate}
          >
            {t("newObject")}
          </Button>
        }
      >
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
      </Panel>
    </Tabs>
  );
};

export default GatewaySectionTable;
