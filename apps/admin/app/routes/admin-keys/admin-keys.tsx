import { useEffect, useState } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Table } from "@/components";
import type { AdminKey } from "@/services/types";
import { useTranslations } from "@/i18n";
import CreateAdminKey from "./create-admin-key";
import EditAdminKey from "./edit-admin-key";
import { columns } from "./columns";

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

  useEffect(() => {
    getAdminKeys();
  }, [getAdminKeys]);

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("adminKeys")}</Title>
        <CreateAdminKey onSave={createAdminKey} />
      </Group>
      <Table
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
