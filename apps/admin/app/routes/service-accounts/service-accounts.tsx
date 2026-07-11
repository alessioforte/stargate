import { useEffect, useState } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { Table, SearchInput } from "@/components";
import { useTranslations } from "@/i18n";
import type { ServiceAccount } from "@/services/types";
import useStore from "@/store";
import { columns } from "./columns";
import CreateServiceAccount from "./create-service-account";
import EditServiceAccount from "./edit-service-account";

const DEFAULT_PAGE_SIZE = 20;

const ServiceAccountsPage = () => {
  const {
    organizations,
    getOrganizations,
    serviceAccounts,
    getServiceAccounts,
    createServiceAccount,
    updateServiceAccount,
    deleteServiceAccount,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedServiceAccount, setSelectedServiceAccount] =
    useState<ServiceAccount | null>(null);
  const [search, setSearch] = useState("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getServiceAccounts({
      q: search || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [getServiceAccounts, search, pagination.pageIndex, pagination.pageSize]);

  useEffect(() => {
    getOrganizations({ limit: 100 });
  }, [getOrganizations]);

  useEffect(() => {
    const total = serviceAccounts?.data?.total;
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
  }, [pagination.pageIndex, pagination.pageSize, serviceAccounts?.data?.total]);

  const handleSearch = (query: string) => {
    setSearch(query);
    setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
  };

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("serviceAccounts")}</Title>
        <SearchInput
          radius="lg"
          variant="unstyled"
          style={{ flex: 1 }}
          placeholder={t("search")}
          onSearch={handleSearch}
        />
        <CreateServiceAccount
          organizations={organizations?.data?.data ?? []}
          onSave={createServiceAccount}
        />
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
          totalItems: serviceAccounts?.data?.total,
        }}
        onPaginationChange={({ pageIndex, pageSize }) => {
          setPagination({ pageIndex, pageSize });
        }}
        columns={columns}
        data={serviceAccounts?.data?.data ?? []}
        empty={serviceAccounts?.data?.data?.length === 0}
        loading={serviceAccounts?.isLoading() ?? false}
        meta={{
          open,
          setSelectedItem: setSelectedServiceAccount,
        }}
      />
      <EditServiceAccount
        opened={opened}
        organizations={organizations?.data?.data ?? []}
        serviceAccount={selectedServiceAccount}
        onClose={() => {
          setSelectedServiceAccount(null);
          close();
        }}
        onSave={updateServiceAccount}
        onDelete={deleteServiceAccount}
      />
    </Box>
  );
};

export default ServiceAccountsPage;
