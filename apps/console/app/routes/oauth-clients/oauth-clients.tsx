import { useEffect, useState } from "react";
import { useDisclosure } from "@mantine/hooks";
import { Page, Panel, Table, SearchInput } from "@/components";
import { useTranslations } from "@/i18n";
import type { OAuthClient } from "@/services/types";
import useStore from "@/store";
import { columns } from "./columns";
import CreateOAuthClient from "./create-oauth-client";
import EditOAuthClient from "./edit-oauth-client";

const DEFAULT_PAGE_SIZE = 20;

const OAuthClientsPage = () => {
  const {
    oauthClients,
    getOAuthClients,
    createOAuthClient,
    updateOAuthClient,
    enableOAuthClient,
    disableOAuthClient,
    deleteOAuthClient,
    rotateOAuthClientSecret,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedOAuthClient, setSelectedOAuthClient] =
    useState<OAuthClient | null>(null);
  const [search, setSearch] = useState("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getOAuthClients({
      q: search || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [getOAuthClients, search, pagination.pageIndex, pagination.pageSize]);

  useEffect(() => {
    const total = oauthClients?.data?.total;
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
  }, [pagination.pageIndex, pagination.pageSize, oauthClients?.data?.total]);

  const handleSearch = (query: string) => {
    setSearch(query);
    setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
  };

  return (
    <Page
      title={t("oauthClients")}
      description={t("pages.oauthClients")}
      actions={<CreateOAuthClient onSave={createOAuthClient} />}
    >
      <Panel
        toolbar={
          <SearchInput placeholder={t("search")} onSearch={handleSearch} />
        }
      >
        <Table
          pagination
          paginationOptions={{
            defaultPageSize: DEFAULT_PAGE_SIZE,
            pageIndex: pagination.pageIndex,
            pageSize: pagination.pageSize,
            totalItems: oauthClients?.data?.total,
          }}
          onPaginationChange={({ pageIndex, pageSize }) => {
            setPagination({ pageIndex, pageSize });
          }}
          columns={columns}
          data={oauthClients?.data?.data ?? []}
          empty={oauthClients?.data?.data?.length === 0}
          loading={oauthClients?.isLoading() ?? false}
          meta={{
            open,
            setSelectedItem: setSelectedOAuthClient,
          }}
        />
      </Panel>
      <EditOAuthClient
        opened={opened}
        oauthClient={selectedOAuthClient}
        onClose={() => {
          setSelectedOAuthClient(null);
          close();
        }}
        onSave={updateOAuthClient}
        onEnable={enableOAuthClient}
        onDisable={disableOAuthClient}
        onDelete={deleteOAuthClient}
        onRotateSecret={rotateOAuthClientSecret}
      />
    </Page>
  );
};

export default OAuthClientsPage;
