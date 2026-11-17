import { useEffect } from "react";
import {
  Alert,
  Anchor,
  Badge,
  Code,
  SimpleGrid,
  Skeleton,
  Stack,
  Text,
} from "@mantine/core";
import { Link } from "react-router";
import {
  LuAppWindow,
  LuBot,
  LuBuilding2,
  LuKeyRound,
  LuMonitorSmartphone,
  LuShieldCheck,
  LuUserCheck,
  LuUsers,
} from "react-icons/lu";
import { Page, Panel } from "@/components";
import useStore from "@/store";
import type { JsonValue, OutboxEvent } from "@/services/types";
import { formatDate } from "@/lib/format-date";
import { useTranslations } from "@/i18n";
import { usePolling } from "@/hooks";
import MetricCard from "./metric-card";
import StatusRow from "./status-row";
import Metric from "./metric";
import DetailRow from "./detail-row";
import classes from "./home.module.css";

const ICON_SIZE = 17;
const auditRelayColor = {
  running: "teal",
  paused: "orange",
  failing: "orange",
  disabled: "gray",
};

export default function AdminHomePage() {
  const t = useTranslations();
  const {
    adminHealth,
    adminMe,
    adminOverview,
    getAdminHealth,
    getAdminOverview,
    getOutboxEvents,
    language,
    outboxEvents,
  } = useStore();

  useEffect(() => {
    void getAdminOverview();
    void getOutboxEvents({ limit: 6 });
  }, [getAdminOverview, getOutboxEvents]);

  const refresh = () => {
    void getAdminHealth();
    void getAdminOverview();
    void getOutboxEvents({ limit: 6 });
  };

  usePolling(refresh, { interval: 9000 });

  const overview = adminOverview.data;
  const health = adminHealth.data;
  const loadingOverview = !overview && !adminOverview.isError();
  const recentActivity = outboxEvents.data?.data ?? [];
  const loadingActivity = !outboxEvents.data && !outboxEvents.isError();
  const firstName = adminMe?.user?.name.split(/\s+/)[0] || t("admin");
  const stateStatus =
    health?.redis?.status ?? (health?.store ? "healthy" : "unknown");
  const features = health?.features;
  const signupModeColor = { open: "teal", invite: "blue", closed: "gray" };
  const stateLatency = health?.redis
    ? `${health.redis.latencyMs.toFixed(1)} ms`
    : health?.store
      ? `${health.store.totalKeys.toLocaleString()} ${t("storedKeys")}`
      : undefined;

  return (
    <Page
      title={t("homeWelcome", { name: firstName })}
      description={t("operationalOverview")}
    >
      {adminOverview.isError() && (
        <Alert color="red" title={t("overviewUnavailable")}>
          {adminOverview.message}
        </Alert>
      )}

      <SimpleGrid cols={{ base: 2, lg: 4 }} spacing="md">
        <MetricCard
          to="/users"
          label={t("users")}
          value={overview?.resources.users.total}
          detail={t("registeredIdentities")}
          loading={loadingOverview}
          icon={<LuUsers size={ICON_SIZE} />}
        />
        <MetricCard
          to="/organizations"
          label={t("organizations")}
          value={overview?.resources.organizations.total}
          detail={t("managedOrganizations")}
          loading={loadingOverview}
          icon={<LuBuilding2 size={ICON_SIZE} />}
        />
        <MetricCard
          to="/sessions"
          label={t("activeSessions")}
          value={overview?.sessions.activeSessions}
          detail={t("openLoginSessions")}
          loading={loadingOverview}
          icon={<LuMonitorSmartphone size={ICON_SIZE} />}
        />
        <MetricCard
          to="/sessions"
          label={t("activeUsers")}
          value={overview?.sessions.activeUsers}
          detail={t("signedInUsers")}
          loading={loadingOverview}
          icon={<LuUserCheck size={ICON_SIZE} />}
        />
        <MetricCard
          to="/service-accounts"
          label={t("serviceAccounts")}
          value={overview?.resources.serviceAccounts.total}
          detail={t("machineIdentities")}
          loading={loadingOverview}
          icon={<LuBot size={ICON_SIZE} />}
        />
        <MetricCard
          to="/oauth-clients"
          label={t("oauthClients")}
          value={overview?.resources.oauthClients.total}
          detail={`${overview?.resources.oauthClients.enabled ?? "—"} ${t("enabled").toLowerCase()}`}
          loading={loadingOverview}
          icon={<LuAppWindow size={ICON_SIZE} />}
        />
        <MetricCard
          to="/api-keys"
          label={t("apiKeys")}
          value={overview?.resources.apiKeys.active}
          detail={`${overview?.resources.apiKeys.revoked ?? "—"} ${t("revoked").toLowerCase()}`}
          loading={loadingOverview}
          icon={<LuKeyRound size={ICON_SIZE} />}
        />
        <MetricCard
          to="/admin-keys"
          label={t("adminKeys")}
          value={overview?.resources.adminKeys.active}
          detail={`${overview?.resources.adminKeys.revoked ?? "—"} ${t("revoked").toLowerCase()}`}
          loading={loadingOverview}
          icon={<LuShieldCheck size={ICON_SIZE} />}
        />
      </SimpleGrid>

      <Panel
        title={t("gatewayRuntime")}
        actions={
          <Anchor component={Link} to="/gateway" size="sm">
            {t("gatewayConfiguration")}
          </Anchor>
        }
      >
        {overview ? (
          <>
            <div className={classes.metrics}>
              <Metric label={t("routers")} value={overview.gateway.routers} />
              <Metric label={t("services")} value={overview.gateway.services} />
              <Metric
                label={t("upstreams")}
                value={overview.gateway.upstreams}
              />
              <Metric label={t("targets")} value={overview.gateway.targets} />
            </div>
            <div className={classes.footnote}>
              {t("runtimeRevision")}: {overview.gateway.configVersion}
              {overview.gateway.policyRevision
                ? ` · ${overview.gateway.policyRevision.slice(0, 20)}…`
                : ""}
            </div>
          </>
        ) : (
          <Skeleton h={90} m="md" />
        )}
      </Panel>

      <div className={classes.columns}>
        <Panel
          title={t("systemHealth")}
          actions={
            health && (
              <Badge color={health.status === "healthy" ? "teal" : "red"}>
                {health.status}
              </Badge>
            )
          }
        >
          {adminHealth.isError() && !health ? (
            <Alert color="red" m="md">
              {adminHealth.message}
            </Alert>
          ) : health ? (
            <div className={classes.rows}>
              <StatusRow
                label={`${t("database")} · ${health.databaseBackend}`}
                status={health.database.status}
                detail={`${health.database.latencyMs.toFixed(1)} ms`}
              />
              <StatusRow
                label={`${t("stateStore")} · ${health.stateBackend}`}
                status={stateStatus}
                detail={stateLatency}
              />
              <div className={classes.footnote}>
                {health.runtimeProfile} · v{health.version} ·{" "}
                {formatDate(health.checkedAt, null, language)}
              </div>
            </div>
          ) : (
            <Stack m="md">
              <Skeleton h={38} />
              <Skeleton h={38} />
            </Stack>
          )}
        </Panel>

        <Panel
          title={t("auditDelivery.title")}
          actions={
            overview && (
              <Badge color="gray">
                {overview.audit.mode.replaceAll("_", " ")}
              </Badge>
            )
          }
        >
          {overview ? (
            <div className={classes.rows}>
              {health?.audit && (
                <StatusRow
                  label={t("status")}
                  status={t(`auditDelivery.states.${health.audit.relay}`)}
                  color={auditRelayColor[health.audit.relay]}
                  detail={
                    health.audit.pauseReason
                      ? t(`auditDelivery.reasons.${health.audit.pauseReason}`)
                      : undefined
                  }
                />
              )}
              <DetailRow label={t("pendingEvents")}>
                {overview.audit.pending.toLocaleString()}
              </DetailRow>
              {overview.audit.oldestPendingAt && (
                <DetailRow label={t("oldestPending")}>
                  {formatDate(overview.audit.oldestPendingAt, null, language)}
                </DetailRow>
              )}
              {health?.audit && (
                <div className={classes.footnote}>
                  {[
                    t("auditDelivery.pending", {
                      count: health.audit.pendingEvents,
                      age: health.audit.oldestPendingAgeSecs.toFixed(1),
                    }),
                    health.audit.streamLength !== undefined &&
                      t("auditDelivery.stream", {
                        count: health.audit.streamLength,
                      }),
                    health.audit.oldestUnconfirmedAgeSecs !== undefined &&
                      t("auditDelivery.unconfirmed", {
                        age: health.audit.oldestUnconfirmedAgeSecs.toFixed(1),
                      }),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </div>
              )}
              {overview.audit.mode === "durable_only" && (
                <div className={classes.footnote}>{t("edgeAuditHint")}</div>
              )}
            </div>
          ) : (
            <Skeleton h={90} m="md" />
          )}
        </Panel>

        <div className={classes.stack}>
          {features && (
            <Panel title={t("signInOptions")}>
              <div className={classes.rows}>
                <StatusRow
                  label={t("signupMode")}
                  status={t(`signupModes.${features.signupMode}`)}
                  color={signupModeColor[features.signupMode]}
                  detail={
                    features.signupAllowedDomains.length > 0
                      ? t("allowedDomains", {
                          domains: features.signupAllowedDomains.join(", "),
                        })
                      : undefined
                  }
                />
                <StatusRow
                  label={t("passwordlessLogin")}
                  status={
                    features.passwordlessLogin ? t("enabled") : t("disabled")
                  }
                  color={features.passwordlessLogin ? "teal" : "gray"}
                />
                <StatusRow
                  label={t("socialLogin")}
                  status={
                    features.socialProviders.length > 0
                      ? features.socialProviders.join(", ")
                      : t("noneConfigured")
                  }
                  color={features.socialProviders.length > 0 ? "teal" : "gray"}
                />
              </div>
            </Panel>
          )}
        </div>
      </div>

      <div className={classes.columns} data-wide>
        <Panel
          title={t("currentSession")}
          actions={
            adminMe && <Badge color="gray">{adminMe.authentication.kind}</Badge>
          }
        >
          {adminMe ? (
            <div className={classes.rows}>
              <DetailRow label={t("sessionId")}>
                {adminMe.authentication.sessionId ? (
                  <Code>{adminMe.authentication.sessionId}</Code>
                ) : (
                  "—"
                )}
              </DetailRow>
              {adminMe.authentication.clientId && (
                <DetailRow label={t("clientId")}>
                  <Code>{adminMe.authentication.clientId}</Code>
                </DetailRow>
              )}
              <DetailRow label={t("authenticatedAt")}>
                {adminMe.authentication.authenticatedAt
                  ? formatDate(
                      adminMe.authentication.authenticatedAt,
                      null,
                      language,
                    )
                  : "—"}
              </DetailRow>
              <DetailRow label={t("accessExpiresAt")}>
                {adminMe.authentication.tokenExpiresAt
                  ? formatDate(
                      adminMe.authentication.tokenExpiresAt,
                      null,
                      language,
                    )
                  : "—"}
              </DetailRow>
              <div className={classes.footnote}>
                <Anchor component={Link} to="/sessions" size="xs">
                  {t("viewAllSessions")}
                </Anchor>
              </div>
            </div>
          ) : (
            <Skeleton h={120} m="md" />
          )}
        </Panel>

        <Panel
          title={t("recentActivity")}
          actions={
            <Anchor component={Link} to="/audits" size="sm">
              {t("viewAllAudits")}
            </Anchor>
          }
        >
          {outboxEvents.isError() && recentActivity.length === 0 ? (
            <Alert color="red" m="md">
              {outboxEvents.message}
            </Alert>
          ) : loadingActivity ? (
            <Stack m="md">
              <Skeleton h={28} />
              <Skeleton h={28} />
              <Skeleton h={28} />
            </Stack>
          ) : recentActivity.length > 0 ? (
            <div className={classes.rows}>
              {recentActivity.map((event) => (
                <div className={classes.row} key={event.eventId}>
                  <div className={classes.rowLabel}>
                    {activityDescription(event, t("auditChange"))}
                    <div className={classes.rowDetail}>
                      <Text span ff="monospace" fz={11}>
                        {event.eventId}
                      </Text>
                    </div>
                  </div>
                  <div className={classes.rowValue}>
                    {formatDate(event.createdAt, null, language)}
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <Text size="sm" c="dimmed" p="md">
              {t("noRecentActivity")}
            </Text>
          )}
        </Panel>
      </div>
    </Page>
  );
}

function asRecord(value: JsonValue): Record<string, JsonValue> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value
    : null;
}

function activityDescription(event: OutboxEvent, fallback: string) {
  const payload = asRecord(event.payload);
  const resource = payload ? asRecord(payload.resource) : null;
  const action =
    typeof payload?.action === "string"
      ? payload.action.replaceAll("_", " ")
      : fallback;
  const resourceType =
    typeof resource?.type === "string"
      ? resource.type.replaceAll("_", " ")
      : null;

  return resourceType ? `${action} · ${resourceType}` : action;
}
