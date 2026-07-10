import {
  ActionIcon,
  Group,
  Table as MantineTable,
  Pagination,
  ScrollArea,
  Select,
  Skeleton,
  Text,
} from "@mantine/core";
import { GoChevronLeft, GoChevronRight } from "react-icons/go";
import {
  type SortingState,
  flexRender,
  getCoreRowModel,
  getExpandedRowModel,
  getPaginationRowModel,
  useReactTable,
} from "@tanstack/react-table";
import React, { useEffect, useState } from "react";
import { Box } from "@mantine/core";
import Empty from "../empty/empty";
import { selectionColumn } from "./rox-checkbox";
import { type TableProps } from "./types";
import classes from "./table.module.css";

export const DEFAULT_PAGE_LIMIT_SMALL = 10;
export const DEFAULT_PAGE_LIMIT_LARGE = 100;

const Table = <T, V>({
  columns,
  data = [],
  pagination,
  meta,
  verticalSpacing = 7,
  paginationOptions,
  enableRowSelection = false,
  stickyHeader = false,
  expandableRows = false,
  loading,
  empty,
  onPaginationChange,
  onSelectionChange,
  onSortingChange,
  defaultSorting,
  enableScrollContainer = false,
  minWidth,
  maxHeight,
}: TableProps<T, V>) => {
  const [rowSelection, setRowSelection] = React.useState({});
  const [sorting, setSorting] = useState<SortingState>(defaultSorting ?? []);
  const [pageIndex, setPageIndex] = React.useState(
    paginationOptions?.pageIndex ?? 0,
  );

  const table = useReactTable({
    columns: [...(enableRowSelection ? selectionColumn : []), ...columns],
    data,
    enableExpanding: expandableRows,
    meta,
    state: {
      rowSelection,
      sorting,
    },
    getCoreRowModel: getCoreRowModel(),
    getExpandedRowModel: getExpandedRowModel(),
    getPaginationRowModel: pagination ? getPaginationRowModel() : undefined,
    onRowSelectionChange: setRowSelection,
    onSortingChange: (sortingUpdater) => {
      setSorting((currentSorting) => {
        let newSorting;

        if (typeof sortingUpdater === "function") {
          newSorting = sortingUpdater(currentSorting);
        } else {
          newSorting = sortingUpdater;
        }

        onSortingChange?.(newSorting);
        return newSorting;
      });
    },
  });
  const { rows } = table.getRowModel();

  useEffect(() => {
    setPageIndex(paginationOptions?.pageIndex ?? 0);
    table.setPageSize(
      paginationOptions?.defaultPageSize ??
        paginationOptions?.pageSize ??
        DEFAULT_PAGE_LIMIT_SMALL,
    );
  }, []);

  useEffect(() => {
    const indexes = Object.keys(rowSelection).map((i) => Number(i));
    const selectedRows = data.filter((_, i) => indexes.includes(i));
    onSelectionChange?.(selectedRows);
  }, [rowSelection]);

  useEffect(() => {
    setPageIndex(paginationOptions?.pageIndex ?? 0);
  }, [paginationOptions?.pageIndex]);

  const totalItems = Number(paginationOptions?.totalItems);
  const pageCount = Number.isFinite(totalItems)
    ? Math.max(1, Math.ceil(totalItems / table.getState().pagination.pageSize))
    : null;
  const showTable = !empty || loading;
  const skeletonRows = Math.max(
    5,
    paginationOptions?.pageSize ?? DEFAULT_PAGE_LIMIT_SMALL,
  );
  const visibleColumns = table.getVisibleLeafColumns();
  const totalColumnSize = Math.max(
    1,
    visibleColumns.reduce(
      (total, column) => total + Math.max(column.getSize(), 1),
      0,
    ),
  );

  const renderColumnGroup = () => (
    <colgroup>
      {visibleColumns.map((column) => (
        <col
          key={column.id}
          style={{
            width: `${(Math.max(column.getSize(), 1) / totalColumnSize) * 100}%`,
          }}
        />
      ))}
    </colgroup>
  );

  const TableBodyContainer = ({
    children,
  }: React.PropsWithChildren): React.ReactElement => {
    if (!enableScrollContainer) {
      return <Box className={classes.tableBody}>{children}</Box>;
    }

    return (
      <ScrollArea
        className={classes.tableBody}
        offsetScrollbars={false}
        type="auto"
      >
        {children}
      </ScrollArea>
    );
  };

  return (
    <>
      <Box className={classes.tableLayout} style={{ height: maxHeight }}>
        <Box className={classes.tableHeader}>
          <MantineTable
            className={classes.table}
            verticalSpacing={verticalSpacing}
            highlightOnHover
            style={{ minWidth }}
          >
            {renderColumnGroup()}
            <MantineTable.Thead className={stickyHeader ? classes.sticky : ""}>
              {React.Children.toArray(
                table.getHeaderGroups().map((headerGroup) => (
                  <MantineTable.Tr key={headerGroup.id}>
                    {React.Children.toArray(
                      headerGroup.headers.map(
                        ({ isPlaceholder, column, getContext }) =>
                          isPlaceholder ? null : (
                            <MantineTable.Th key={column.id}>
                              {flexRender(column.columnDef.header, {
                                ...getContext(),
                              })}
                            </MantineTable.Th>
                          ),
                      ),
                    )}
                  </MantineTable.Tr>
                )),
              )}
            </MantineTable.Thead>
          </MantineTable>
        </Box>

        <TableBodyContainer>
          {showTable && (
            <MantineTable
              className={classes.table}
              verticalSpacing={verticalSpacing}
              highlightOnHover
              style={{ minWidth }}
            >
              {renderColumnGroup()}
              <MantineTable.Tbody>
                {loading &&
                  Array.from({ length: skeletonRows }, (_, rowIndex) => (
                    <MantineTable.Tr key={`skeleton-${rowIndex}`}>
                      {visibleColumns.map((column) => (
                        <MantineTable.Td key={column.id}>
                          <Skeleton height={28} radius="sm" />
                        </MantineTable.Td>
                      ))}
                    </MantineTable.Tr>
                  ))}
                {!loading &&
                  React.Children.toArray(
                    rows.map((row) => (
                      <MantineTable.Tr key={row.id}>
                        {React.Children.toArray(
                          row.getVisibleCells().map((cell) => (
                            <MantineTable.Td key={cell.column.id}>
                              {flexRender(cell.column.columnDef.cell, {
                                ...cell.getContext(),
                              })}
                            </MantineTable.Td>
                          )),
                        )}
                      </MantineTable.Tr>
                    )),
                  )}
              </MantineTable.Tbody>
            </MantineTable>
          )}

          {empty && !loading && (
            <Box className={classes.emptyState}>
              <Empty style={{ height: "100%" }} />
            </Box>
          )}
        </TableBodyContainer>

        {pagination && (
          <Group className={classes.pagination}>
            {pageCount !== null && (
              <Pagination
                size="sm"
                total={pageCount}
                value={pageIndex + 1}
                onChange={(value) => {
                  setPageIndex(value - 1);
                  onPaginationChange?.({
                    pageIndex: value - 1,
                    pageSize: table.getState().pagination.pageSize,
                    sorting: table.getState().sorting,
                  });
                }}
              />
            )}
            {pageCount === null && (
              <Group gap={6}>
                <ActionIcon
                  size={26}
                  variant="default"
                  disabled={pageIndex === 0}
                  onClick={() => {
                    setPageIndex(pageIndex - 1);
                    onPaginationChange?.({
                      pageIndex: pageIndex - 1,
                      pageSize: table.getState().pagination.pageSize,
                      sorting: table.getState().sorting,
                    });
                  }}
                >
                  <GoChevronLeft size={11} />
                </ActionIcon>
                <ActionIcon size={26} variant="filled" color="primary">
                  <Text size="xs" fw={500}>
                    {pageIndex + 1}
                  </Text>
                </ActionIcon>
                <ActionIcon
                  disabled={
                    data.length === 0 ||
                    data.length < table.getState().pagination.pageSize
                  }
                  size={26}
                  variant="default"
                  onClick={() => {
                    setPageIndex(pageIndex + 1);
                    onPaginationChange?.({
                      pageIndex: pageIndex + 1,
                      pageSize: table.getState().pagination.pageSize,
                      sorting: table.getState().sorting,
                    });
                  }}
                >
                  <GoChevronRight size={11} />
                </ActionIcon>
              </Group>
            )}
            <Select
              size="xs"
              variant="unstyled"
              value={String(table.getState().pagination.pageSize)}
              autoComplete="off"
              style={{ maxWidth: 70 }}
              onChange={(value) => {
                if (!value) return;
                table.setPageSize(Number(value));
                setPageIndex(0);
                onPaginationChange?.({
                  pageIndex: 0,
                  pageSize: Number(value),
                  sorting: table.getState().sorting,
                });
              }}
              data={[
                { value: "10", label: "10" },
                { value: "15", label: "15" },
                { value: "20", label: "20" },
                { value: "50", label: "50" },
                { value: "100", label: "100" },
              ]}
            />
          </Group>
        )}
      </Box>
    </>
  );
};

export default Table;
