import { useState, useEffect } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Table, SearchInput } from "@/components";
import type { User } from "@/services/types";
import { useTranslations } from "@/i18n";
import CreateUser from "./create-user";
import EditUser from "./edit-user";
import { columns } from "./columns";

const DEFAULT_PAGE_SIZE = 20;

const UsersPage = () => {
  const {
    users,
    organizations,
    superAdminUsers,
    getUsers,
    getOrganizations,
    getSuperAdminUsers,
    createUser,
    inviteUser,
    updateUser,
    deleteUser,
    updateUserAttrs,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedUser, setSelectedUser] = useState<User | null>(null);
  const [search, setSearch] = useState("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getUsers({
      q: search || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [getUsers, search, pagination.pageIndex, pagination.pageSize]);

  useEffect(() => {
    getSuperAdminUsers();
  }, [getSuperAdminUsers]);

  useEffect(() => {
    getOrganizations({ limit: 100 });
  }, [getOrganizations]);

  useEffect(() => {
    const total = users?.data?.total;
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
  }, [pagination.pageIndex, pagination.pageSize, users?.data?.total]);

  const handleSearch = (query: string) => {
    setSearch(query);
    setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
  };

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("users")}</Title>
        <SearchInput
          radius="lg"
          variant="unstyled"
          style={{ flex: 1 }}
          placeholder={`${t("search").toLowerCase()}...`}
          onSearch={handleSearch}
        />
        <CreateUser
          organizations={organizations?.data?.data ?? []}
          onSave={createUser}
          onInvite={inviteUser}
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
          totalItems: users?.data?.total,
        }}
        onPaginationChange={({ pageIndex, pageSize }) => {
          setPagination({ pageIndex, pageSize });
        }}
        columns={columns}
        data={users?.data?.data ?? []}
        empty={users?.data?.data?.length === 0}
        loading={users?.isLoading() ?? false}
        meta={{
          open,
          superAdminUsers: superAdminUsers?.data ?? [],
          setSelectedItem: setSelectedUser,
        }}
      />
      <EditUser
        opened={opened}
        user={selectedUser}
        onClose={() => {
          setSelectedUser(null);
          close();
        }}
        onSave={updateUser}
        onDelete={deleteUser}
        onAttrsSave={updateUserAttrs}
      />
    </Box>
  );
};

export default UsersPage;
