import { useState, useEffect } from "react";
import { Box, Group, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Table } from "@/components";
import type { Organization } from "@/services/types";
import { useTranslations } from "@/i18n";
import CreateOrganization from "./create-organization";
import EditOrganization from "./edit-organization";
import { columns } from "./columns";

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

  useEffect(() => {
    getOrganizations();
  }, [getOrganizations]);

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("organizations")}</Title>
        <CreateOrganization onSave={createOrganization} />
      </Group>
      <Table
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
