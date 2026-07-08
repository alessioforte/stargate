import {
  type Cell,
  type ColumnDef,
  type SortingState,
  type TableMeta,
} from "@tanstack/react-table";

export interface TableProps<T, V> {
  columns: ColumnDef<T, V>[];
  data: T[];
  meta?: TableMeta<T>;
  containerRef?: React.RefObject<HTMLDivElement>;
  verticalSpacing?: "xs" | "sm" | "md" | "lg" | "xl" | number;
  pagination?: boolean;
  paginationOptions?: TablePaginationOptions;
  enableRowSelection?: boolean;
  onPaginationChange?: (pagination: TablePaginationProps) => void;
  onSelectionChange?: (selectedRows: T[]) => void;
  onSortingChange?: (sorting: SortingState) => void;
  defaultSorting?: SortingState;
  stickyHeader?: boolean;
  expandableRows?: boolean;
  loading?: boolean;
  empty?: boolean;
}

export type TableCellProps<T, V> = Cell<T, V> & { additionalProps: any };

export interface TablePaginationProps {
  pageIndex: number;
  pageSize: number;
  sorting?: SortingState;
}

export interface TablePaginationOptions {
  defaultPageSize?: number;
  pageIndex?: number;
  pageSize?: number;
  pageCount?: number;
  totalItems?: number;
}
