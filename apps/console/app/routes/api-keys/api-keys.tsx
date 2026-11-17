import { useEffect, useState } from "react";
import { useDisclosure } from "@mantine/hooks";
import { Page, Panel, Table, SearchInput } from "@/components";
import { useTranslations } from "@/i18n";
import type { ApiKey } from "@/services/types";
import useStore from "@/store";
import { columns } from "./columns";
import CreateApiKey from "./create-api-key";
import EditApiKey from "./edit-api-key";

const DEFAULT_PAGE_SIZE = 20;

const APIKeysPage = () => {
  const {
    apiKeys,
    getApiKeys,
    createApiKey,
    updateApiKeyAttrs,
    revokeApiKey,
    deleteApiKey,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedApiKey, setSelectedApiKey] = useState<ApiKey | null>(null);
  const [search, setSearch] = useState("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getApiKeys({
      q: search || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [getApiKeys, search, pagination.pageIndex, pagination.pageSize]);

  useEffect(() => {
    const total = apiKeys?.data?.total;
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
  }, [pagination.pageIndex, pagination.pageSize, apiKeys?.data?.total]);

  const handleSearch = (query: string) => {
    setSearch(query);
    setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
  };

  return (
    <Page
      title={t("apiKeys")}
      description={t("pages.apiKeys")}
      actions={<CreateApiKey onSave={createApiKey} />}
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
            totalItems: apiKeys?.data?.total,
          }}
          onPaginationChange={({ pageIndex, pageSize }) => {
            setPagination({ pageIndex, pageSize });
          }}
          columns={columns}
          data={apiKeys?.data?.data ?? []}
          empty={apiKeys?.data?.data?.length === 0}
          loading={apiKeys?.isLoading() ?? false}
          meta={{
            open,
            setSelectedItem: setSelectedApiKey,
          }}
        />
      </Panel>
      <EditApiKey
        opened={opened}
        apiKey={selectedApiKey}
        onClose={() => {
          setSelectedApiKey(null);
          close();
        }}
        onAttrsSave={updateApiKeyAttrs}
        onRevoke={revokeApiKey}
        onDelete={deleteApiKey}
      />
    </Page>
  );
};

export default APIKeysPage;
