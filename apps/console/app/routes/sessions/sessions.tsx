import { useEffect, useState } from "react";
import { Alert } from "@mantine/core";
import { Page, Panel, SearchInput, Table, useConfirmModal } from "@/components";
import { useTranslations } from "@/i18n";
import type { AdminSession, ListAdminSessionsQuery } from "@/services/types";
import useStore from "@/store";
import { usePolling } from "@/hooks";
import { columns } from "./columns";

const DEFAULT_PAGE_SIZE = 20;

export default function SessionsPage() {
  const {
    adminSessions,
    getAdminSessions,
    revokeAdminSession,
    revokeUserSessions,
  } = useStore();
  const t = useTranslations();
  const { confirm, confirmModal } = useConfirmModal();
  const [filters, setFilters] = useState<ListAdminSessionsQuery>({});
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    void getAdminSessions({
      ...filters,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [filters, getAdminSessions, pagination.pageIndex, pagination.pageSize]);

  useEffect(() => {
    const total = adminSessions.data?.total;
    if (total === undefined) return;

    const lastPageIndex = Math.max(
      0,
      Math.ceil(total / pagination.pageSize) - 1,
    );
    if (pagination.pageIndex > lastPageIndex) {
      setPagination((current) => ({
        ...current,
        pageIndex: lastPageIndex,
      }));
    }
  }, [adminSessions.data?.total, pagination.pageIndex, pagination.pageSize]);

  const updateFilter = (
    name: "clientId" | "organizationId" | "userId",
    value: string,
  ) => {
    setFilters((current) => ({
      ...current,
      [name]: value.trim() || undefined,
    }));
    setPagination((current) => ({ ...current, pageIndex: 0 }));
  };

  const confirmSessionRevocation = async (session: AdminSession) => {
    const confirmed = await confirm({
      title: t("revokeSession"),
      confirmLabel: t("revoke"),
      message: session.current
        ? t("confirmRevokeCurrentSession")
        : t("confirmRevokeSession", {
            user: session.user.name ?? session.user.email ?? session.user.id,
          }),
    });
    if (confirmed) {
      await revokeAdminSession(session.id);
    }
  };

  const confirmUserRevocation = async (session: AdminSession) => {
    const confirmed = await confirm({
      title: t("revokeAllUserSessions"),
      confirmLabel: t("revokeAll"),
      message: t("confirmRevokeAllUserSessions", {
        user: session.user.name ?? session.user.email ?? session.user.id,
      }),
    });
    if (confirmed) {
      await revokeUserSessions(session.user.id);
    }
  };

  const refresh = () => {
    void getAdminSessions({
      ...filters,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  };

  usePolling(refresh, { interval: 5000 });

  return (
    <Page title={t("sessions")} description={t("pages.sessions")}>
      {adminSessions.isError() && (
        <Alert color="red">{adminSessions.message}</Alert>
      )}

      <Panel
        toolbar={
          <>
            <SearchInput
              w={220}
              placeholder={t("filterByUserId")}
              onSearch={(value) => updateFilter("userId", value)}
            />
            <SearchInput
              w={220}
              placeholder={t("filterByOrganizationId")}
              onSearch={(value) => updateFilter("organizationId", value)}
            />
            <SearchInput
              w={220}
              placeholder={t("filterByClientId")}
              onSearch={(value) => updateFilter("clientId", value)}
            />
          </>
        }
      >
        <Table
          minWidth={1660}
          pagination
          paginationOptions={{
            defaultPageSize: DEFAULT_PAGE_SIZE,
            pageIndex: pagination.pageIndex,
            pageSize: pagination.pageSize,
            totalItems: adminSessions.data?.total,
          }}
          onPaginationChange={({ pageIndex, pageSize }) => {
            setPagination({ pageIndex, pageSize });
          }}
          columns={columns}
          data={adminSessions.data?.data ?? []}
          empty={adminSessions.data?.data?.length === 0}
          meta={{
            onRevokeSession: (session: AdminSession) => {
              void confirmSessionRevocation(session);
            },
            onRevokeUserSessions: (session: AdminSession) => {
              void confirmUserRevocation(session);
            },
          }}
        />
      </Panel>

      {confirmModal}
    </Page>
  );
}
