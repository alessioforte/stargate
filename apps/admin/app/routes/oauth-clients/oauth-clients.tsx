import { useEffect, useState } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { Table } from "@/components";
import { useTranslations } from "@/i18n";
import type { OAuthClient } from "@/services/types";
import useStore from "@/store";
import { columns } from "./columns";
import CreateOAuthClient from "./create-oauth-client";
import EditOAuthClient from "./edit-oauth-client";

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

  useEffect(() => {
    getOAuthClients();
  }, [getOAuthClients]);

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("oauthClients")}</Title>
        <CreateOAuthClient onSave={createOAuthClient} />
      </Group>
      <Table
        columns={columns}
        data={oauthClients?.data?.data ?? []}
        empty={oauthClients?.data?.data?.length === 0}
        loading={oauthClients?.isLoading() ?? false}
        meta={{
          open,
          setSelectedItem: setSelectedOAuthClient,
        }}
      />
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
    </Box>
  );
};

export default OAuthClientsPage;
