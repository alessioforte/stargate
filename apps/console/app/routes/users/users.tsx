import { useState, useEffect } from "react";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Page, Panel, Table, SearchInput } from "@/components";
import type { User } from "@/services/types";
import { useTranslations } from "@/i18n";
import CreateUser from "./create-user";
import EditUser from "./edit-user";
import { columns } from "./columns";

const DEFAULT_PAGE_SIZE = 20;

const UsersPage = () => {
  const {
    adminHealth,
    getAdminHealth,
    users,
    organizations,
    oauthClients,
    superAdminUsers,
    getUsers,
    getOrganizations,
    getOAuthClients,
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

  // The signup mode decides whether invitations are offered. Until it is
  // known (or without permission to read it) they are, and the server has
  // the last word.
  const healthIdle = adminHealth.isIdle();
  useEffect(() => {
    if (healthIdle) void getAdminHealth();
  }, [healthIdle, getAdminHealth]);
  const invitationsEnabled = adminHealth.data?.features.signupMode !== "closed";

  useEffect(() => {
    getOrganizations({ limit: 100 });
  }, [getOrganizations]);

  // The invitation form can send the user back to an app that starts the
  // login (OpenID Connect Core 1.0 §4). Without permission to list clients
  // the select stays empty and the invitation goes without an app.
  useEffect(() => {
    getOAuthClients({ limit: 100 });
  }, [getOAuthClients]);

  const apps = (oauthClients?.data?.data ?? [])
    .filter((client) => client.enabled && client.initiateLoginUri)
    .map((client) => ({ id: client.clientId, name: client.name }));

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
    <Page
      title={t("users")}
      description={t("pages.users")}
      actions={
        <CreateUser
          organizations={organizations?.data?.data ?? []}
          apps={apps}
          onSave={createUser}
          onInvite={inviteUser}
          invitationsEnabled={invitationsEnabled}
        />
      }
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
      </Panel>
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
    </Page>
  );
};

export default UsersPage;
