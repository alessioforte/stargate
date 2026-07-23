import { useEffect, useState } from "react";
import { Box, Group, Select, TextInput, Title } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Table } from "@/components";
import type {
  OutboxEvent,
  OutboxEventPairRole,
  OutboxEventStatus,
} from "@/services/types";
import { useTranslations } from "@/i18n";
import { columns } from "./columns";
import OutboxEventDetail from "./event-detail";

const DEFAULT_PAGE_SIZE = 20;

const pairRoleOptions = [
  { value: "target", label: "Target" },
  { value: "control_plane", label: "Control Plane" },
] as const;

const statusOptions = [
  { value: "pending", label: "Pending" },
  { value: "published", label: "Published" },
] as const;

const AuditsPage = () => {
  const { outboxEvents, getOutboxEvents } = useStore();
  const t = useTranslations();

  const [opened, { open, close }] = useDisclosure(false);
  const [selectedEvent, setSelectedEvent] = useState<OutboxEvent | null>(null);
  const [eventIdFilter, setEventIdFilter] = useState("");
  const [operationIdFilter, setOperationIdFilter] = useState("");
  const [pairRoleFilter, setPairRoleFilter] = useState<
    OutboxEventPairRole | ""
  >("");
  const [statusFilter, setStatusFilter] = useState<OutboxEventStatus | "">("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getOutboxEvents({
      eventId: eventIdFilter || undefined,
      operationId: operationIdFilter || undefined,
      pairRole: pairRoleFilter || undefined,
      status: statusFilter || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [
    getOutboxEvents,
    eventIdFilter,
    operationIdFilter,
    pairRoleFilter,
    statusFilter,
    pagination.pageIndex,
    pagination.pageSize,
  ]);

  useEffect(() => {
    const total = outboxEvents?.data?.total;
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
  }, [pagination.pageIndex, pagination.pageSize, outboxEvents?.data?.total]);

  const handleCloseDetail = () => {
    setSelectedEvent(null);
    close();
  };

  return (
    <Box>
      <Group p="xs" justify="space-between">
        <Title order={4}>{t("audits")}</Title>
      </Group>

      <Group p="xs" gap="xs" wrap="wrap">
        <TextInput
          size="xs"
          style={{ width: 260 }}
          variant="filled"
          placeholder={t("eventId")}
          value={eventIdFilter}
          onChange={(event) => {
            setEventIdFilter(event.currentTarget.value);
            setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
          }}
        />
        <TextInput
          size="xs"
          style={{ width: 260 }}
          variant="filled"
          placeholder={t("operationId")}
          value={operationIdFilter}
          onChange={(event) => {
            setOperationIdFilter(event.currentTarget.value);
            setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
          }}
        />
        <Select
          size="xs"
          style={{ width: 160 }}
          clearable
          variant="filled"
          placeholder={t("pairRole")}
          data={pairRoleOptions}
          value={pairRoleFilter || null}
          onChange={(value) => {
            setPairRoleFilter((value ?? "") as OutboxEventPairRole | "");
            setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
          }}
        />
        <Select
          size="xs"
          style={{ width: 140 }}
          clearable
          variant="filled"
          placeholder={t("status")}
          data={statusOptions}
          value={statusFilter || null}
          onChange={(value) => {
            setStatusFilter((value ?? "") as OutboxEventStatus | "");
            setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
          }}
        />
      </Group>

      <Table
        stickyHeader
        enableScrollContainer
        maxHeight="calc(var(--page-height) - 100px)"
        pagination
        paginationOptions={{
          defaultPageSize: DEFAULT_PAGE_SIZE,
          pageIndex: pagination.pageIndex,
          pageSize: pagination.pageSize,
          totalItems: outboxEvents?.data?.total,
        }}
        onPaginationChange={({ pageIndex, pageSize }) => {
          setPagination({ pageIndex, pageSize });
        }}
        meta={{
          open,
          setSelectedItem: (event: OutboxEvent | null) => {
            setSelectedEvent(event);
          },
        }}
        columns={columns}
        data={outboxEvents?.data?.data ?? []}
        empty={outboxEvents?.data?.data?.length === 0}
        loading={outboxEvents?.isLoading() ?? false}
      />

      <OutboxEventDetail
        event={selectedEvent}
        opened={opened}
        onClose={handleCloseDetail}
      />
    </Box>
  );
};

export default AuditsPage;
