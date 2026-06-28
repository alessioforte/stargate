import { useEffect, useState } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { Table } from "@/components";
import { useTranslations } from "@/i18n";
import type { ApiKey } from "@/services/types";
import useStore from "@/store";
import { columns } from "./columns";
import CreateApiKey from "./create-api-key";
import EditApiKey from "./edit-api-key";

const APIKeysPage = () => {
  const {
    apiKeys,
    users,
    serviceAccounts,
    getApiKeys,
    getUsers,
    getServiceAccounts,
    createApiKey,
    updateApiKeyAttrs,
    revokeApiKey,
    deleteApiKey,
  } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedApiKey, setSelectedApiKey] = useState<ApiKey | null>(null);

  useEffect(() => {
    getApiKeys();
    getUsers({ limit: 100 });
    getServiceAccounts({ limit: 100 });
  }, [getApiKeys, getServiceAccounts, getUsers]);

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("apiKeys")}</Title>
        <CreateApiKey
          users={users?.data?.data ?? []}
          serviceAccounts={serviceAccounts?.data?.data ?? []}
          onSave={createApiKey}
        />
      </Group>
      <Table
        columns={columns}
        data={apiKeys?.data?.data ?? []}
        empty={apiKeys?.data?.data?.length === 0}
        loading={apiKeys?.isLoading() ?? false}
        meta={{
          open,
          setSelectedItem: setSelectedApiKey,
        }}
      />
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
    </Box>
  );
};

export default APIKeysPage;
