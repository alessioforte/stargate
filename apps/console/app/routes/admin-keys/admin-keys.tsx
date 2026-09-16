import { useEffect, useState } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Table, SearchInput } from "@/components";
import type { AdminKey } from "@/services/types";
import { useTranslations } from "@/i18n";
import CreateAdminKey from "./create-admin-key";
import EditAdminKey from "./edit-admin-key";
import { columns } from "./columns";

const DEFAULT_PAGE_SIZE = 20;

const AdminKeysPage = () => {
  const {
    adminKeys,
    getAdminKeys,
    createAdminKey,
    updateAdminKeyPermissions,
    revokeAdminKey,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedAdminKey, setSelectedAdminKey] = useState<AdminKey | null>(
    null,
  );
  const [search, setSearch] = useState("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getAdminKeys({
      q: search || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [getAdminKeys, search, pagination.pageIndex, pagination.pageSize]);

  useEffect(() => {
    const total = adminKeys?.data?.total;
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
  }, [pagination.pageIndex, pagination.pageSize, adminKeys?.data?.total]);

  const handleSearch = (query: string) => {
    setSearch(query);
    setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
  };

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("adminKeys")}</Title>
        <SearchInput
          radius="lg"
          variant="unstyled"
          style={{ flex: 1 }}
          placeholder={t("search")}
          onSearch={handleSearch}
        />
        <CreateAdminKey onSave={createAdminKey} />
      </Group>
      <Table
        stickyHeader
        enableScrollContainer
        maxHeight="calc(var(--page-height) - 60px)"
        pagination
        paginationOptions={{
          defaultPageSize: DEFAULT_PAGE_SIZE,
          pageIndex: pagination.pageIndex,
          pageSize: pagination.pageSize,
          totalItems: adminKeys?.data?.total,
        }}
        onPaginationChange={({ pageIndex, pageSize }) => {
          setPagination({ pageIndex, pageSize });
        }}
        columns={columns}
        data={adminKeys?.data?.data ?? []}
        empty={adminKeys?.data?.data?.length === 0}
        loading={adminKeys?.isLoading() ?? false}
        meta={{
          open,
          setSelectedItem: setSelectedAdminKey,
        }}
      />
      <EditAdminKey
        opened={opened}
        adminKey={selectedAdminKey}
        onClose={() => {
          setSelectedAdminKey(null);
          close();
        }}
        onSave={updateAdminKeyPermissions}
        onRevoke={revokeAdminKey}
      />
    </Box>
  );
};

export default AdminKeysPage;
