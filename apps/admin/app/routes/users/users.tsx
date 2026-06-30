import { useState, useEffect } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Table } from "@/components";
import type { User } from "@/services/types";
import { useTranslations } from "@/i18n";
import CreateUser from "./create-user";
import EditUser from "./edit-user";
import { columns } from "./columns";

const UsersPage = () => {
  const {
    users,
    getUsers,
    createUser,
    updateUser,
    deleteUser,
    updateUserAttrs,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedUser, setSelectedUser] = useState<User | null>(null);

  useEffect(() => {
    getUsers();
  }, [getUsers]);

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("users")}</Title>
        <CreateUser onSave={createUser} />
      </Group>
      <Table
        columns={columns}
        data={users?.data?.data ?? []}
        empty={users?.data?.data?.length === 0}
        loading={users?.isLoading() ?? false}
        meta={{
          open,
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
