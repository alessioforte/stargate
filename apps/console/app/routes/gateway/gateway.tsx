import { useEffect } from "react";
import { Alert, Box, Stack } from "@mantine/core";
import { useConfirmModal } from "@/components";
import { useTranslations } from "@/i18n";
import useStore from "@/store";
import GatewayEditorDrawer from "./gateway-editor-drawer";
import GatewaySectionTable from "./gateway-section-table";
import GatewayToolbar from "./gateway-toolbar";
import { useGatewayConfigBuilder } from "./use-gateway-config-builder";

const GatewayPage = () => {
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();

  const { configuration, getConfigurations, updateConfigurations } = useStore();

  const savedConfig = configuration.data;
  const builder = useGatewayConfigBuilder(savedConfig);

  useEffect(() => {
    getConfigurations();
  }, [getConfigurations]);

  const handleReload = async () => {
    if (builder.dirty) {
      const confirmed = await confirm({
        color: "red",
        confirmLabel: t("discardChanges"),
        message: t("confirmDiscardChanges"),
        title: t("unsavedChanges"),
      });
      if (!confirmed) return;
    }

    getConfigurations();
  };

  const handleSave = async () => {
    if (!builder.config) return;

    const saved = await updateConfigurations(builder.config);
    if (saved) {
      getConfigurations();
    }
  };

  return (
    <Box>
      {confirmModal}
      <GatewayToolbar
        dirty={builder.dirty}
        loading={configuration.isLoading()}
        saveDisabled={!builder.dirty || !builder.config}
        schema={builder.schema}
        onReload={handleReload}
        onReset={builder.resetDraft}
        onSave={handleSave}
      />

      <Stack mt="xs" gap="sm">
        {configuration.isError() && (
          <Alert color="red" variant="light">
            {configuration.message}
          </Alert>
        )}

        <GatewaySectionTable
          loading={configuration.isLoading()}
          rows={builder.selectedRows}
          sectionCounts={builder.sectionCounts}
          selectedSectionKey={builder.selectedSectionKey}
          onCreate={builder.createObject}
          onOpen={builder.openEditor}
          onSectionChange={builder.selectSection}
          onSelect={builder.selectObject}
        />

        <GatewayEditorDrawer
          config={builder.config}
          drawerMode={builder.drawerMode}
          existingNames={builder.selectedNames}
          opened={builder.drawerOpened}
          selectedName={builder.editorName}
          selectedSection={builder.selectedSection}
          selectedValue={builder.editorValue}
          onApply={builder.applyObject}
          onClose={builder.closeEditor}
          onDelete={builder.deleteObject}
        />
      </Stack>
    </Box>
  );
};

export default GatewayPage;
