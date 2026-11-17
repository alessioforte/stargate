import { useEffect, useState } from "react";
import { Select, TextInput } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import useStore from "@/store";
import { Page, Panel, Table } from "@/components";
import type { OutboxEvent, OutboxEventStatus } from "@/services/types";
import { useTranslations } from "@/i18n";
import { columns } from "./columns";
import OutboxEventDetail from "./event-detail";

const DEFAULT_PAGE_SIZE = 20;

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
  const [statusFilter, setStatusFilter] = useState<OutboxEventStatus | "">("");
  const [pagination, setPagination] = useState({
    pageIndex: 0,
    pageSize: DEFAULT_PAGE_SIZE,
  });

  useEffect(() => {
    getOutboxEvents({
      eventId: eventIdFilter || undefined,
      operationId: operationIdFilter || undefined,
      status: statusFilter || undefined,
      limit: pagination.pageSize,
      offset: pagination.pageIndex * pagination.pageSize,
    });
  }, [
    getOutboxEvents,
    eventIdFilter,
    operationIdFilter,
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
    <Page title={t("audits")} description={t("pages.audits")}>
      <Panel
        toolbar={
          <>
            <TextInput
              style={{ width: 260 }}
              placeholder={t("eventId")}
              value={eventIdFilter}
              onChange={(event) => {
                setEventIdFilter(event.currentTarget.value);
                setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
              }}
            />
            <TextInput
              style={{ width: 260 }}
              placeholder={t("operationId")}
              value={operationIdFilter}
              onChange={(event) => {
                setOperationIdFilter(event.currentTarget.value);
                setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
              }}
            />
            <Select
              style={{ width: 140 }}
              clearable
              placeholder={t("status")}
              data={statusOptions}
              value={statusFilter || null}
              onChange={(value) => {
                setStatusFilter((value ?? "") as OutboxEventStatus | "");
                setPagination({ pageIndex: 0, pageSize: DEFAULT_PAGE_SIZE });
              }}
            />
          </>
        }
      >
        <Table
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
      </Panel>

      <OutboxEventDetail
        event={selectedEvent}
        opened={opened}
        onClose={handleCloseDetail}
      />
    </Page>
  );
};

export default AuditsPage;
