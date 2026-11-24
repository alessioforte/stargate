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
  const [pageSize, setPageSize] = useState(DEFAULT_PAGE_SIZE);
  // The cursor of every page visited so far; the first page has none.
  const [cursors, setCursors] = useState<(string | undefined)[]>([undefined]);
  const pageIndex = cursors.length - 1;
  const cursor = cursors[pageIndex];
  const nextCursor = adminSessions.data?.nextCursor ?? null;

  useEffect(() => {
    void getAdminSessions({ ...filters, limit: pageSize, cursor });
  }, [cursor, filters, getAdminSessions, pageSize]);

  const updateFilter = (
    name: "clientId" | "organizationId" | "userId",
    value: string,
  ) => {
    setFilters((current) => ({
      ...current,
      [name]: value.trim() || undefined,
    }));
    setCursors([undefined]);
  };

  const changePage = (next: { pageIndex: number; pageSize: number }) => {
    if (next.pageSize !== pageSize) {
      setPageSize(next.pageSize);
      setCursors([undefined]);
    } else if (next.pageIndex > pageIndex && nextCursor) {
      setCursors((current) => [...current, nextCursor]);
    } else if (next.pageIndex < pageIndex) {
      setCursors((current) => current.slice(0, next.pageIndex + 1));
    }
  };

  const confirmSessionRevocation = async (session: AdminSession) => {
    const confirmed = await confirm({
      title: t("revokeSession"),
      confirmLabel: t("revoke"),
      message: session.current
        ? t("confirmRevokeCurrentSession")
        : t("confirmRevokeSession", { user: session.user.name }),
    });
    if (confirmed) {
      await revokeAdminSession(session.id);
    }
  };

  const confirmUserRevocation = async (session: AdminSession) => {
    const confirmed = await confirm({
      title: t("revokeAllUserSessions"),
      confirmLabel: t("revokeAll"),
      message: t("confirmRevokeAllUserSessions", { user: session.user.name }),
    });
    if (confirmed) {
      await revokeUserSessions(session.user.id);
    }
  };

  const refresh = () => {
    void getAdminSessions({ ...filters, limit: pageSize, cursor });
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
            pageIndex,
            pageSize,
            hasNextPage: nextCursor !== null,
          }}
          onPaginationChange={changePage}
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
