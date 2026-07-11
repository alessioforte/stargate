import { useState, useEffect } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Table, SearchInput } from "@/components";
import type { Organization } from "@/services/types";
import { useTranslations } from "@/i18n";
import CreateOrganization from "./create-organization";
import EditOrganization from "./edit-organization";
import { columns } from "./columns";

const DEFAULT_PAGE_SIZE = 20;

const OrganizationsPage = () => {
  const {
    organizations,
    getOrganizations,
    createOrganization,
    updateOrganization,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedOrganization, setSelectedOrganization] =
    useState<Organization | null>(null);
  const [search, setSearch] = useState("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getOrganizations({
      q: search || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [getOrganizations, search, pagination.pageIndex, pagination.pageSize]);

  useEffect(() => {
    const total = organizations?.data?.total;
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
  }, [pagination.pageIndex, pagination.pageSize, organizations?.data?.total]);

  const handleSearch = (query: string) => {
    setSearch(query);
    setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
  };

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("organizations")}</Title>
        <SearchInput
          radius="lg"
          variant="unstyled"
          style={{ flex: 1 }}
          placeholder={t("search")}
          onSearch={handleSearch}
        />
        <CreateOrganization onSave={createOrganization} />
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
          totalItems: organizations?.data?.total,
        }}
        onPaginationChange={({ pageIndex, pageSize }) => {
          setPagination({ pageIndex, pageSize });
        }}
        columns={columns}
        data={organizations?.data?.data ?? []}
        empty={organizations?.data?.data?.length === 0}
        loading={organizations?.isLoading() ?? false}
        meta={{
          open,
          setSelectedItem: setSelectedOrganization,
        }}
      />
      <EditOrganization
        opened={opened}
        organization={selectedOrganization}
        onClose={() => {
          setSelectedOrganization(null);
          close();
        }}
        onSave={updateOrganization}
      />
    </Box>
  );
};

export default OrganizationsPage;
