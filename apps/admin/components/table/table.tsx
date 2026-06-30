import {
  ActionIcon,
  Group,
  Table as MantineTable,
  Pagination,
  Select,
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
import { Box, LoadingOverlay } from "@mantine/core";
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
  verticalSpacing = "xs",
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

  const loadingOverlayZIndex = 1;

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

  const pageCount =
    paginationOptions?.totalItems &&
    !Number.isNaN(Number(paginationOptions.totalItems))
      ? Math.ceil(
          Number(paginationOptions.totalItems) /
            table.getState().pagination.pageSize,
        )
      : null;

  return (
    <>
      <Box pos="relative">
        <LoadingOverlay
          visible={loading && !empty}
          zIndex={loadingOverlayZIndex}
        />
        <MantineTable
          className={classes.table}
          verticalSpacing={verticalSpacing}
          stickyHeader={stickyHeader}
          highlightOnHover
          withColumnBorders
        >
          {!empty && (
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
          )}

          {!empty && (
            <MantineTable.Tbody>
              {React.Children.toArray(
                rows.map((row) => (
                  <MantineTable.Tr key={row.id}>
                    {React.Children.toArray(
                      row.getVisibleCells().map((cell) => (
                        <MantineTable.Td
                          key={cell.column.id}
                          style={{ width: cell.column.columnDef.size }}
                        >
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
          )}
        </MantineTable>

        {empty && (
          <Box style={{ height: "300px" }}>
            {!loading && (
              <Empty
                style={{
                  height: "100%",
                }}
              />
            )}
            {loading && (
              <LoadingOverlay visible zIndex={loadingOverlayZIndex} />
            )}
          </Box>
        )}
      </Box>
      {pagination && (
        <Group mt={30}>
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
    </>
  );
};

export default Table;
