import type { ReactNode } from "react";

/// A form input kind. `switch` can map a boolean to arbitrary on/off values
/// (e.g. status 1/2) via `switchOn`/`switchOff`.
export type FieldType =
  | "text"
  | "number"
  | "color"
  | "avatar"
  | "relation"
  | "oauth_provider"
  | "password"
  | "switch"
  | "select"
  | "expiration"
  | "strategy_options"
  | "tag_names"
  | "textarea";

export interface SelectOption {
  label: string; // i18n key or literal
  value: string | number;
}

export interface RelationDef {
  api: string | ((form: Record<string, unknown>) => string);
  valueField?: string;
  labelFields?: string[];
  params?: Record<string, unknown> | ((form: Record<string, unknown>) => Record<string, unknown>);
  includeEmptyOption?: boolean;
  emptyLabel?: string;
  valueAsString?: boolean;
}

export interface FieldDef {
  name: string;
  label: string; // i18n key
  type: FieldType;
  options?: SelectOption[];
  relation?: RelationDef;
  tagHistory?: RelationDef;
  hint?: string; // i18n key
  placeholder?: string; // i18n key
  visibleWhen?: (form: Record<string, unknown>, editing: boolean) => boolean;
  /** Only shown when creating (e.g. password). */
  createOnly?: boolean;
  /** Disabled when editing (e.g. username). */
  lockOnEdit?: boolean;
  defaultValue?: unknown;
  /** For `switch` fields backing a non-boolean column. */
  switchOn?: unknown;
  switchOff?: unknown;
  /** Field used to derive the default RustDesk tag color. */
  colorSeedField?: string;
  /** Fields reset to their default value when this field changes. */
  resetFieldsOnChange?: string[];
}

export interface ColumnDef<T = Record<string, unknown>> {
  key: string;
  label: string; // i18n key
  render?: (row: T, t: (k: string) => string) => ReactNode;
}

export interface RowActionHelpers<T = Record<string, unknown>> {
  openDelete: (row: T) => void;
}

export interface FilterDef {
  name: string; // query-param name
  label: string; // i18n key (placeholder)
}

export interface ResourceConfig<T = Record<string, unknown>> {
  /** Route segment + query key. */
  name: string;
  /** Optional absolute route path, used for legacy-compatible nested paths. */
  path?: string;
  /** i18n key for the page title + sidebar label. */
  titleKey: string;
  /** API base, e.g. `/api/admin/user`. */
  api: string;
  /** Primary key field on the row + in delete payloads ("id" | "row_id"). */
  idField?: string;
  columns: ColumnDef<T>[];
  fields: FieldDef[];
  filters?: FilterDef[];
  rowActions?: (
    row: T,
    t: (key: string) => string,
    helpers: RowActionHelpers<T>,
  ) => ReactNode;
  canCreate?: boolean;
  canEdit?: boolean;
  canDelete?: boolean;
}
