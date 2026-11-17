import { useEffect, useMemo, useState } from "react";
import { Alert, Badge, Group, Stack, Tabs } from "@mantine/core";
import {
  Page,
  Panel,
  RevisionConflictDialog,
  Table,
  useConfirmModal,
} from "@/components";
import { useTranslations } from "@/i18n";
import useStore from "@/store";
import type {
  AccessControlRule,
  EvaluateAccessControlCapabilitiesRequest,
  EvaluateAccessControlRequest,
} from "@/services/types";
import AccessControlToolbar from "./access-control-toolbar";
import CapabilityEvaluator from "./capability-evaluator";
import { accessControlRuleColumns } from "./columns";
import DocumentEditor from "./document-editor";
import RuleEvaluator from "./rule-evaluator";

const AccessControlPolicies = () => {
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();

  const {
    accessControlRules,
    accessControlRulesConflict,
    language,
    resolveActorEmail,
    refreshAccessControlRulesConflict,
    reloadAccessControlRulesConflict,
    overwriteAccessControlRules,
    accessControlValidation,
    accessControlEvaluation,
    accessControlCapabilitiesEvaluation,
    getAccessControlRules,
    updateAccessControlRules,
    validateAccessControlRules,
    evaluateAccessControlRules,
    evaluateAccessControlCapabilities,
  } = useStore();

  const saved = accessControlRules.data ?? null;
  const [content, setContent] = useState("");
  const [syncedRevision, setSyncedRevision] = useState<string | null>(null);
  const [validatedContent, setValidatedContent] = useState<string | null>(null);

  useEffect(() => {
    getAccessControlRules();
  }, [getAccessControlRules]);

  useEffect(() => {
    if (!saved || saved.revision === syncedRevision) return;

    setContent(saved.content);
    setSyncedRevision(saved.revision);
    setValidatedContent(saved.content);
  }, [saved, syncedRevision]);

  const dirty = saved ? content !== saved.content : false;
  const activeValidation =
    validatedContent === content ? accessControlValidation.data : null;

  const rows: AccessControlRule[] = useMemo(() => {
    if (activeValidation) return activeValidation.rules;
    if (!dirty) return saved?.rules ?? [];
    return [];
  }, [activeValidation, dirty, saved]);

  const diagnostics = useMemo(() => {
    if (activeValidation) return activeValidation.diagnostics;
    if (!dirty) return saved?.diagnostics ?? [];
    return [];
  }, [activeValidation, dirty, saved]);

  const valid =
    activeValidation?.valid ??
    (!dirty ? (saved?.valid ?? undefined) : undefined);

  const handleReload = async () => {
    if (dirty) {
      const confirmed = await confirm({
        color: "red",
        confirmLabel: t("discardChanges"),
        message: t("confirmDiscardChanges"),
        title: t("unsavedChanges"),
      });
      if (!confirmed) return;
    }

    if (saved) {
      setContent(saved.content);
      setValidatedContent(saved.content);
    }
    getAccessControlRules();
  };

  const handleReset = () => {
    if (!saved) return;
    setContent(saved.content);
    setValidatedContent(saved.content);
  };

  const handleValidate = async () => {
    const result = await validateAccessControlRules({ content });
    if (result) {
      setValidatedContent(content);
    }
  };

  const handleSave = async () => {
    if (!saved || activeValidation?.valid !== true) return;

    const result = await updateAccessControlRules({
      content,
      revision: saved.revision,
    });

    if (result) {
      setContent(result.content);
      setSyncedRevision(result.revision);
      setValidatedContent(result.content);
    }
  };

  const handleEvaluate = async (request: EvaluateAccessControlRequest) => {
    return evaluateAccessControlRules(request);
  };

  const handleEvaluateCapabilities = async (
    request: EvaluateAccessControlCapabilitiesRequest,
  ) => {
    return evaluateAccessControlCapabilities(request);
  };

  return (
    <Page
      title={t("accessControlPolicies")}
      description={t("pages.accessControl")}
      badges={
        <>
          {saved?.revision && (
            <Badge color="gray" ff="monospace">
              {saved.revision.slice(0, 19)}
            </Badge>
          )}
          {valid !== undefined && (
            <Badge color={valid ? "teal" : "red"}>
              {valid ? t("valid") : t("invalid")}
            </Badge>
          )}
          {dirty && <Badge color="orange">{t("unsavedChanges")}</Badge>}
        </>
      }
      actions={
        <AccessControlToolbar
          dirty={dirty}
          loading={accessControlRules.isLoading()}
          saveDisabled={
            !dirty || valid !== true || accessControlRules.isLoading()
          }
          validating={accessControlValidation.isLoading()}
          onReload={handleReload}
          onReset={handleReset}
          onSave={handleSave}
          onValidate={handleValidate}
        />
      }
    >
      {confirmModal}
      <RevisionConflictDialog
        opened={accessControlRulesConflict !== null}
        language={language}
        actorEmail={resolveActorEmail(
          accessControlRulesConflict?.data?.updatedBy,
        )}
        loading={
          accessControlRules.isLoading() ||
          accessControlRulesConflict?.isLoading() === true
        }
        error={
          accessControlRules.isError()
            ? (accessControlRules.message ?? t("loadLatestFailed"))
            : accessControlRulesConflict?.isError()
              ? (accessControlRulesConflict.message ?? t("loadLatestFailed"))
              : undefined
        }
        latest={accessControlRulesConflict?.data}
        onReload={() => {
          const latest = reloadAccessControlRulesConflict();
          if (latest) {
            setContent(latest.content);
            setSyncedRevision(latest.revision);
            setValidatedContent(latest.content);
          }
        }}
        onOverwrite={() => {
          void overwriteAccessControlRules(content);
        }}
        onRetry={refreshAccessControlRulesConflict}
      />
      {accessControlRules.isError() && (
        <Alert color="red">{accessControlRules.message}</Alert>
      )}

      <Tabs defaultValue="list">
        <Tabs.List mb="md">
          <Tabs.Tab value="list">
            <Group gap={6}>
              {t("rules")}
              <Badge size="sm" color="gray">
                {rows.length}
              </Badge>
            </Group>
          </Tabs.Tab>
          <Tabs.Tab value="document">{t("document")}</Tabs.Tab>
          <Tabs.Tab value="evaluator">{t("evaluator")}</Tabs.Tab>
          <Tabs.Tab value="capabilities">{t("capabilities")}</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="list">
          <Stack gap="sm">
            {dirty && !activeValidation && (
              <Alert color="yellow">{t("validateToRefreshRules")}</Alert>
            )}
            {diagnostics.length > 0 && (
              <Alert color="red">{t("rulesInvalid")}</Alert>
            )}
            <Panel>
              <Table
                columns={accessControlRuleColumns}
                data={rows}
                empty={rows.length === 0}
                loading={
                  accessControlRules.isLoading() ||
                  accessControlValidation.isLoading()
                }
              />
            </Panel>
          </Stack>
        </Tabs.Panel>

        <Tabs.Panel value="document">
          <Panel>
            <DocumentEditor
              content={content}
              diagnostics={diagnostics}
              onChange={(value) => {
                setContent(value);
              }}
            />
          </Panel>
        </Tabs.Panel>

        <Tabs.Panel value="evaluator">
          <Panel padded>
            <RuleEvaluator
              error={accessControlEvaluation.message}
              loading={accessControlEvaluation.isLoading()}
              result={accessControlEvaluation.data}
              onEvaluate={handleEvaluate}
            />
          </Panel>
        </Tabs.Panel>

        <Tabs.Panel value="capabilities">
          <Panel padded>
            <CapabilityEvaluator
              error={
                accessControlCapabilitiesEvaluation.isError()
                  ? accessControlCapabilitiesEvaluation.message
                  : undefined
              }
              loading={accessControlCapabilitiesEvaluation.isLoading()}
              result={
                accessControlCapabilitiesEvaluation.isSuccess()
                  ? accessControlCapabilitiesEvaluation.data
                  : null
              }
              onEvaluate={handleEvaluateCapabilities}
            />
          </Panel>
        </Tabs.Panel>
      </Tabs>
    </Page>
  );
};

export default AccessControlPolicies;
