import { useEffect } from "react";
import { Alert, Badge } from "@mantine/core";
import {
  Page,
  Panel,
  RevisionConflictDialog,
  useConfirmModal,
} from "@/components";
import { useTranslations } from "@/i18n";
import useStore from "@/store";
import GatewayEditorDrawer from "./gateway-editor-drawer";
import GatewaySectionTable from "./gateway-section-table";
import GatewayToolbar from "./gateway-toolbar";
import GatewayIngressSettings from "./gateway-ingress-settings";
import { GATEWAY_SCHEMA } from "./config-utils";
import { useGatewayConfigBuilder } from "./use-gateway-config-builder";

const GatewayPage = () => {
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();

  const {
    configuration,
    configurationConflict,
    language,
    resolveActorEmail,
    getConfigurations,
    updateConfigurations,
    refreshConfigurationConflict,
    reloadConfigurationConflict,
    overwriteConfigurations,
  } = useStore();

  const savedConfig = configuration.data?.config;
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

    await updateConfigurations(builder.config);
  };

  return (
    <Page
      title={t("gatewayConfiguration")}
      description={t("pages.gateway")}
      badges={
        <>
          {builder.schema && (
            <Badge
              color={builder.schema === GATEWAY_SCHEMA ? "gray" : "yellow"}
            >
              {builder.schema}
            </Badge>
          )}
          {builder.dirty && <Badge color="orange">{t("unsavedChanges")}</Badge>}
        </>
      }
      actions={
        <GatewayToolbar
          dirty={builder.dirty}
          loading={configuration.isLoading()}
          saveDisabled={!builder.dirty || !builder.config}
          onReload={handleReload}
          onReset={builder.resetDraft}
          onSave={handleSave}
        />
      }
    >
      {confirmModal}
      <RevisionConflictDialog
        opened={configurationConflict !== null}
        language={language}
        actorEmail={resolveActorEmail(configurationConflict?.data?.updatedBy)}
        loading={
          configuration.isLoading() ||
          configurationConflict?.isLoading() === true
        }
        error={
          configuration.isError()
            ? (configuration.message ?? t("loadLatestFailed"))
            : configurationConflict?.isError()
              ? (configurationConflict.message ?? t("loadLatestFailed"))
              : undefined
        }
        latest={configurationConflict?.data}
        onReload={reloadConfigurationConflict}
        onOverwrite={() => {
          if (builder.config) void overwriteConfigurations(builder.config);
        }}
        onRetry={refreshConfigurationConflict}
      />
      {configuration.isError() && (
        <Alert color="red">{configuration.message}</Alert>
      )}

      <Panel title={t("ingress")} padded>
        <GatewayIngressSettings
          config={builder.config}
          onChange={builder.updateIngress}
        />
      </Panel>

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
    </Page>
  );
};

export default GatewayPage;
