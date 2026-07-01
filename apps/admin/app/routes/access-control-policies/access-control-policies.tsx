import { useEffect, useMemo, useState } from "react";
import { Alert, Badge, Box, Group, Paper, Stack, Tabs } from "@mantine/core";
import { Table, useConfirmModal } from "@/components";
import { useTranslations } from "@/i18n";
import useStore from "@/store";
import type {
  AccessControlRule,
  EvaluateAccessControlRequest,
} from "@/services/types";
import AccessControlToolbar from "./access-control-toolbar";
import { accessControlRuleColumns } from "./columns";
import DocumentEditor from "./document-editor";
import RuleEvaluator from "./rule-evaluator";

const AccessControlPolicies = () => {
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();

  const {
    accessControlRules,
    accessControlValidation,
    accessControlEvaluation,
    getAccessControlRules,
    updateAccessControlRules,
    validateAccessControlRules,
    evaluateAccessControlRules,
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

  return (
    <Box>
      {confirmModal}
      <AccessControlToolbar
        dirty={dirty}
        loading={accessControlRules.isLoading()}
        revision={saved?.revision}
        saveDisabled={
          !dirty || valid !== true || accessControlRules.isLoading()
        }
        valid={valid}
        validating={accessControlValidation.isLoading()}
        onReload={handleReload}
        onReset={handleReset}
        onSave={handleSave}
        onValidate={handleValidate}
      />

      <Stack mt="xs" gap="sm">
        {accessControlRules.isError() && (
          <Alert color="red" variant="light">
            {accessControlRules.message}
          </Alert>
        )}

        <Tabs defaultValue="list">
          <Tabs.List>
            <Tabs.Tab value="list">
              <Group gap={6}>
                {t("rules")}
                <Badge size="sm" variant="light">
                  {rows.length}
                </Badge>
              </Group>
            </Tabs.Tab>
            <Tabs.Tab value="document">{t("document")}</Tabs.Tab>
            <Tabs.Tab value="evaluator">{t("evaluator")}</Tabs.Tab>
          </Tabs.List>

          <Tabs.Panel value="list">
            <Stack gap="sm">
              {dirty && !activeValidation && (
                <Alert m="xs" color="yellow">
                  {t("validateToRefreshRules")}
                </Alert>
              )}
              {diagnostics.length > 0 && (
                <Alert m="xs" color="red">
                  {t("rulesInvalid")}
                </Alert>
              )}
              <Table
                columns={accessControlRuleColumns}
                data={rows}
                empty={rows.length === 0}
                loading={
                  accessControlRules.isLoading() ||
                  accessControlValidation.isLoading()
                }
              />
            </Stack>
          </Tabs.Panel>

          <Tabs.Panel value="document">
            <DocumentEditor
              content={content}
              diagnostics={diagnostics}
              dirty={dirty}
              valid={valid}
              onChange={(value) => {
                setContent(value);
              }}
            />
          </Tabs.Panel>

          <Tabs.Panel value="evaluator">
            <RuleEvaluator
              error={accessControlEvaluation.message}
              loading={accessControlEvaluation.isLoading()}
              result={accessControlEvaluation.data}
              onEvaluate={handleEvaluate}
            />
          </Tabs.Panel>
        </Tabs>
      </Stack>
    </Box>
  );
};

export default AccessControlPolicies;
