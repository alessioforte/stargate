import { EntityDrawer } from "@/components";
import { useTranslations } from "@/i18n";
import type { JsonValue } from "@/services/types";
import type { GatewaySection } from "./config-utils";
import GatewayObjectEditor from "./gateway-object-editor";
import type { DrawerMode } from "./use-gateway-config-builder";

interface Props {
  config: JsonValue | null;
  drawerMode: DrawerMode;
  existingNames: string[];
  opened: boolean;
  selectedName: string;
  selectedSection: GatewaySection;
  selectedValue: JsonValue | null;
  onApply: (previousName: string, nextName: string, value: JsonValue) => void;
  onClose: () => void;
  onDelete: (name: string) => void;
}

const GatewayEditorDrawer: React.FC<Props> = ({
  config,
  drawerMode,
  existingNames,
  opened,
  selectedName,
  selectedSection,
  selectedValue,
  onApply,
  onClose,
  onDelete,
}) => {
  const t = useTranslations();

  const title =
    drawerMode === "create"
      ? `${t("newObject")}: ${t(selectedSection.labelKey)}`
      : selectedName
        ? `${t(selectedSection.labelKey)}: ${selectedName}`
        : t(selectedSection.labelKey);

  return (
    <EntityDrawer opened={opened} onClose={onClose} title={title}>
      <GatewayObjectEditor
        canDelete={drawerMode === "edit"}
        config={config}
        existingNames={existingNames}
        selectedName={selectedName}
        selectedSection={selectedSection}
        selectedValue={selectedValue}
        onApply={onApply}
        onDelete={onDelete}
      />
    </EntityDrawer>
  );
};

export default GatewayEditorDrawer;
