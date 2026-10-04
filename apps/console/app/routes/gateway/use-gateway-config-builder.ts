import { useCallback, useEffect, useMemo, useState } from "react";
import { useDisclosure } from "@mantine/hooks";
import type { JsonValue } from "@/services/types";
import {
  gatewaySections,
  getPathRecord,
  getSchema,
  stringifyJson,
  type JsonRecord,
} from "./config-utils";
import {
  deleteSectionObject,
  nextObjectName,
  normalizeGatewayConfig,
  renameSectionObject,
} from "./draft-utils";
import type { GatewayObjectRow } from "./gateway-table-utils";
import { getGatewayObjectRows } from "./gateway-table-utils";

export type DrawerMode = "create" | "edit";

export function useGatewayConfigBuilder(savedConfig: JsonValue | undefined) {
  const [selectedSectionKey, setSelectedSectionKey] = useState("routers");
  const [selectedName, setSelectedName] = useState("");
  const [draftObjectName, setDraftObjectName] = useState("");
  const [drawerMode, setDrawerMode] = useState<DrawerMode>("edit");
  const [draftConfig, setDraftConfig] = useState<JsonValue | null>(null);
  const [drawerOpened, { open: openDrawer, close: closeDrawer }] =
    useDisclosure(false);

  const selectedSection =
    gatewaySections.find((section) => section.key === selectedSectionKey) ??
    gatewaySections[0];

  const selectedRecord = useMemo(
    () => getPathRecord(draftConfig, selectedSection.path),
    [draftConfig, selectedSection.path],
  );

  const selectedNames = useMemo(
    () => Object.keys(selectedRecord).sort(),
    [selectedRecord],
  );

  const selectedRows = useMemo(
    () => getGatewayObjectRows(draftConfig, selectedSection),
    [draftConfig, selectedSection],
  );

  const sectionCounts = useMemo(
    () =>
      Object.fromEntries(
        gatewaySections.map((section) => [
          section.key,
          getGatewayObjectRows(draftConfig, section).length,
        ]),
      ),
    [draftConfig],
  );

  const schema = getSchema(draftConfig);
  const dirty = stringifyJson(savedConfig) !== stringifyJson(draftConfig);

  useEffect(() => {
    if (savedConfig === undefined) return;
    setDraftConfig(normalizeGatewayConfig(savedConfig));
  }, [savedConfig]);

  useEffect(() => {
    if (selectedNames.includes(selectedName)) return;
    setSelectedName(selectedNames[0] ?? "");
  }, [selectedName, selectedNames]);

  const selectSection = useCallback((value: string | null) => {
    if (!value) return;
    setSelectedSectionKey(value);
  }, []);

  const resetDraft = useCallback(() => {
    setDraftConfig(normalizeGatewayConfig(savedConfig));
  }, [savedConfig]);

  const updateIngress = useCallback((ingress: JsonRecord) => {
    setDraftConfig((current) => ({
      ...normalizeGatewayConfig(current),
      ingress,
    }));
  }, []);

  const closeEditor = useCallback(() => {
    setDraftObjectName("");
    setDrawerMode("edit");
    closeDrawer();
  }, [closeDrawer]);

  const createObject = useCallback(() => {
    const name = nextObjectName(selectedNames, `new-${selectedSection.key}`);
    setDraftObjectName(name);
    setDrawerMode("create");
    openDrawer();
  }, [openDrawer, selectedNames, selectedSection.key]);

  const selectObject = useCallback((row: GatewayObjectRow | null) => {
    setDraftObjectName("");
    setDrawerMode("edit");
    setSelectedName(row?.name ?? "");
  }, []);

  const applyObject = useCallback(
    (previousName: string, nextName: string, value: JsonValue) => {
      setDraftConfig((current) =>
        renameSectionObject(
          current,
          selectedSection,
          previousName,
          nextName,
          value,
        ),
      );
      setSelectedName(nextName);
      closeEditor();
    },
    [closeEditor, selectedSection],
  );

  const deleteObject = useCallback(
    (name: string) => {
      if (drawerMode === "create") {
        closeEditor();
        return;
      }

      setDraftConfig((current) =>
        deleteSectionObject(current, selectedSection, name),
      );
      setSelectedName("");
      closeEditor();
    },
    [closeEditor, drawerMode, selectedSection],
  );

  const editorName = drawerMode === "create" ? draftObjectName : selectedName;
  const editorValue =
    drawerMode === "create"
      ? null
      : selectedName
        ? selectedRecord[selectedName]
        : null;

  return {
    applyObject,
    closeEditor,
    config: draftConfig,
    createObject,
    deleteObject,
    dirty,
    drawerMode,
    drawerOpened,
    editorName,
    editorValue,
    openEditor: openDrawer,
    resetDraft,
    schema,
    sectionCounts,
    selectObject,
    selectSection,
    selectedNames,
    selectedRows,
    selectedSection,
    selectedSectionKey,
    updateIngress,
  };
}
