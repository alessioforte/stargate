import { useEffect } from "react";
import {
  Alert,
  Anchor,
  Badge,
  Box,
  Card,
  Code,
  Divider,
  Group,
  SimpleGrid,
  Skeleton,
  Stack,
  Text,
  ThemeIcon,
  Title,
} from "@mantine/core";
import { Link } from "react-router";
import {
  HiOutlineKey,
  HiOutlineServerStack,
  HiOutlineUserGroup,
  HiOutlineUsers,
} from "react-icons/hi2";
import { LuActivity, LuMonitorSmartphone, LuRoute } from "react-icons/lu";
import { GoOrganization, GoShieldLock } from "react-icons/go";
import { MdOutlineApps } from "react-icons/md";
import useStore from "@/store";
import type { JsonValue, OutboxEvent } from "@/services/types";
import { formatDate } from "@/lib/format-date";
import { useTranslations } from "@/i18n";
import { usePolling } from "@/hooks";
import MetricCard from "./metric-card";
import StatusRow from "./status-row";
import Metric from "./metric";

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
  const stateLatency = health?.redis
    ? `${health.redis.latencyMs.toFixed(1)} ms`
    : health?.store
      ? `${health.store.totalKeys.toLocaleString()} ${t("storedKeys")}`
      : undefined;

  return (
    <Box p={{ base: "sm", md: "lg" }}>
      <Group justify="space-between" mb="lg">
        <div>
          <Title order={3}>{t("homeWelcome", { name: firstName })}</Title>
          <Text c="dimmed" size="sm" mt={3}>
            {t("operationalOverview")}
          </Text>
        </div>
      </Group>

      {adminOverview.isError() && (
        <Alert color="red" mb="md" title={t("overviewUnavailable")}>
          {adminOverview.message}
        </Alert>
      )}

      <SimpleGrid cols={{ base: 1, xs: 2, md: 4, xl: 8 }} mb="md">
        <MetricCard
          label={t("users")}
          value={overview?.resources.users.total}
          detail={t("registeredIdentities")}
          loading={loadingOverview}
          icon={<HiOutlineUsers size={20} />}
        />
        <MetricCard
          label={t("organizations")}
          value={overview?.resources.organizations.total}
          detail={t("managedOrganizations")}
          loading={loadingOverview}
          icon={<GoOrganization size={20} />}
        />
        <MetricCard
          label={t("serviceAccounts")}
          value={overview?.resources.serviceAccounts.total}
          detail={t("machineIdentities")}
          loading={loadingOverview}
          icon={<HiOutlineServerStack size={20} />}
        />
        <MetricCard
          label={t("oauthClients")}
          value={overview?.resources.oauthClients.total}
          detail={`${overview?.resources.oauthClients.enabled ?? "—"} ${t("enabled").toLowerCase()}`}
          loading={loadingOverview}
          icon={<MdOutlineApps size={20} />}
        />
        <MetricCard
          label={t("apiKeys")}
          value={overview?.resources.apiKeys.active}
          detail={`${overview?.resources.apiKeys.revoked ?? "—"} ${t("revoked").toLowerCase()}`}
          loading={loadingOverview}
          icon={<HiOutlineKey size={20} />}
        />
        <MetricCard
          label={t("adminKeys")}
          value={overview?.resources.adminKeys.active}
          detail={`${overview?.resources.adminKeys.revoked ?? "—"} ${t("revoked").toLowerCase()}`}
          loading={loadingOverview}
          icon={<GoShieldLock size={20} />}
        />
        <MetricCard
          label={t("activeSessions")}
          value={overview?.sessions.activeSessions}
          detail={t("openLoginSessions")}
          loading={loadingOverview}
          icon={<LuMonitorSmartphone size={20} />}
        />
        <MetricCard
          label={t("activeUsers")}
          value={overview?.sessions.activeUsers}
          detail={t("signedInUsers")}
          loading={loadingOverview}
          icon={<HiOutlineUserGroup size={20} />}
        />
      </SimpleGrid>

      <Card withBorder radius="md" mb="md" p={8}>
        <Group gap="xs" mb="md">
          <ThemeIcon variant="light" color="blue">
            <LuRoute size={18} />
          </ThemeIcon>
          <Title order={5}>{t("gatewayRuntime")}</Title>
        </Group>
        {overview ? (
          <Stack>
            <SimpleGrid cols={4}>
              <Metric label={t("routers")} value={overview.gateway.routers} />
              <Metric label={t("services")} value={overview.gateway.services} />
              <Metric
                label={t("upstreams")}
                value={overview.gateway.upstreams}
              />
              <Metric label={t("targets")} value={overview.gateway.targets} />
            </SimpleGrid>
            <Divider />
            <Text size="xs" c="dimmed">
              {t("runtimeRevision")}: {overview.gateway.configVersion}
              {overview.gateway.policyRevision
                ? ` · ${overview.gateway.policyRevision.slice(0, 20)}…`
                : ""}
            </Text>
          </Stack>
        ) : (
          <Skeleton h={90} />
        )}
      </Card>

      <SimpleGrid cols={{ base: 1, lg: 3 }} mb="md">
        <Card withBorder radius="md" p={8}>
          <Group justify="space-between" mb="md">
            <Group gap="xs">
              <ThemeIcon variant="light" color="teal">
                <LuActivity size={18} />
              </ThemeIcon>
              <Title order={5}>{t("systemHealth")}</Title>
            </Group>
            {health && (
              <Badge color={health.status === "healthy" ? "teal" : "red"}>
                {health.status}
              </Badge>
            )}
          </Group>

          {adminHealth.isError() && !health ? (
            <Alert color="red">{adminHealth.message}</Alert>
          ) : health ? (
            <Stack gap="sm">
              <StatusRow
                label={`${t("database")} · ${health.databaseBackend}`}
                status={health.database.status}
                detail={`${health.database.latencyMs.toFixed(1)} ms`}
              />
              <Divider />
              <StatusRow
                label={`${t("stateStore")} · ${health.stateBackend}`}
                status={stateStatus}
                detail={stateLatency}
              />
              <Divider />
              <Group justify="space-between">
                <Text size="xs" c="dimmed">
                  {health.runtimeProfile} · v{health.version}
                </Text>
                <Text size="xs" c="dimmed">
                  {formatDate(health.checkedAt, null, language)}
                </Text>
              </Group>
            </Stack>
          ) : (
            <Stack>
              <Skeleton h={38} />
              <Skeleton h={38} />
            </Stack>
          )}
        </Card>

        <Card withBorder radius="md" p={8}>
          <Group gap="xs" mb="md">
            <ThemeIcon variant="light" color="yellow">
              <LuActivity size={18} />
            </ThemeIcon>
            <Title order={5}>{t("auditDelivery")}</Title>
          </Group>
          {overview ? (
            <Stack gap="sm">
              <Group justify="space-between">
                <Text size="sm">{t("deliveryMode")}</Text>
                <Badge variant="light">
                  {overview.audit.mode.replaceAll("_", " ")}
                </Badge>
              </Group>
              <Group justify="space-between">
                <Text size="sm">{t("pendingEvents")}</Text>
                <Text fw={700}>{overview.audit.pending.toLocaleString()}</Text>
              </Group>
              {overview.audit.oldestPendingAt && (
                <Group justify="space-between">
                  <Text size="sm">{t("oldestPending")}</Text>
                  <Text size="sm" c="dimmed">
                    {formatDate(overview.audit.oldestPendingAt, null, language)}
                  </Text>
                </Group>
              )}
              {overview.audit.mode === "durable_only" && (
                <Text size="xs" c="dimmed">
                  {t("edgeAuditHint")}
                </Text>
              )}
            </Stack>
          ) : (
            <Skeleton h={90} />
          )}
        </Card>

        <Card withBorder radius="md" p={8}>
          <Group justify="space-between" mb="md">
            <Group gap="xs">
              <ThemeIcon variant="light" color="indigo">
                <LuMonitorSmartphone size={18} />
              </ThemeIcon>
              <Title order={5}>{t("currentSession")}</Title>
            </Group>
            {adminMe && (
              <Badge variant="light" color="indigo">
                {adminMe.authentication.kind}
              </Badge>
            )}
          </Group>
          {adminMe ? (
            <Stack gap="sm">
              <Group justify="space-between" wrap="nowrap">
                <Text size="sm">{t("sessionId")}</Text>
                {adminMe.authentication.sessionId ? (
                  <Code fz={10}>{adminMe.authentication.sessionId}</Code>
                ) : (
                  <Text c="dimmed">—</Text>
                )}
              </Group>
              {adminMe.authentication.clientId && (
                <Group justify="space-between" wrap="nowrap">
                  <Text size="sm">{t("clientId")}</Text>
                  <Code fz={10}>{adminMe.authentication.clientId}</Code>
                </Group>
              )}
              <Group justify="space-between" wrap="nowrap">
                <Text size="sm">{t("authenticatedAt")}</Text>
                <Text size="xs" c="dimmed">
                  {adminMe.authentication.authenticatedAt
                    ? formatDate(
                        adminMe.authentication.authenticatedAt,
                        null,
                        language,
                      )
                    : "—"}
                </Text>
              </Group>
              <Group justify="space-between" wrap="nowrap">
                <Text size="sm">{t("accessExpiresAt")}</Text>
                <Text size="xs" c="dimmed">
                  {adminMe.authentication.tokenExpiresAt
                    ? formatDate(
                        adminMe.authentication.tokenExpiresAt,
                        null,
                        language,
                      )
                    : "—"}
                </Text>
              </Group>
              <Anchor component={Link} to="/sessions" size="sm">
                {t("viewAllSessions")}
              </Anchor>
            </Stack>
          ) : (
            <Skeleton h={120} />
          )}
        </Card>
      </SimpleGrid>

      <Card withBorder radius="md" p={8}>
        <Group justify="space-between" mb="md">
          <Title order={5}>{t("recentActivity")}</Title>
          <Anchor component={Link} to="/audits" size="sm">
            {t("viewAllAudits")}
          </Anchor>
        </Group>
        {outboxEvents.isError() && recentActivity.length === 0 ? (
          <Alert color="red">{outboxEvents.message}</Alert>
        ) : loadingActivity ? (
          <Stack>
            <Skeleton h={32} />
            <Skeleton h={32} />
            <Skeleton h={32} />
          </Stack>
        ) : recentActivity.length > 0 ? (
          <Stack gap={0}>
            {recentActivity.map((event, index) => (
              <Box key={event.eventId}>
                {index > 0 && <Divider />}
                <Group justify="space-between" py="sm" wrap="nowrap">
                  <div>
                    <Text size="sm" fw={500}>
                      {activityDescription(event, t("auditChange"))}
                    </Text>
                    <Text size="xs" c="dimmed">
                      {event.eventId}
                    </Text>
                  </div>
                  <Text size="xs" c="dimmed" ta="right">
                    {formatDate(event.createdAt, null, language)}
                  </Text>
                </Group>
              </Box>
            ))}
          </Stack>
        ) : (
          <Text size="sm" c="dimmed">
            {t("noRecentActivity")}
          </Text>
        )}
      </Card>
    </Box>
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
