import { useEffect, useState } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { Table } from "@/components";
import { useTranslations } from "@/i18n";
import type { ServiceAccount } from "@/services/types";
import useStore from "@/store";
import { columns } from "./columns";
import CreateServiceAccount from "./create-service-account";
import EditServiceAccount from "./edit-service-account";

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

  useEffect(() => {
    getServiceAccounts();
    getOrganizations({ limit: 100 });
  }, [getOrganizations, getServiceAccounts]);

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("serviceAccounts")}</Title>
        <CreateServiceAccount
          organizations={organizations?.data?.data ?? []}
          onSave={createServiceAccount}
        />
      </Group>
      <Table
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
