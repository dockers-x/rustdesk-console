import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Switch } from "@cloudflare/kumo/components/switch";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "../components/DialogLayout";
import { TableState } from "../components/TableState";
import { usePublicAdminConfig } from "../lib/adminTitle";
import { apiGet, apiPost } from "../lib/api";
import type { FieldDef, ResourceConfig } from "./types";

interface ListResult {
  list: Record<string, unknown>[];
  total: number;
  page: number;
  page_size: number;
}

const PAGE_SIZE = 10;

function initialForm(cfg: ResourceConfig, row?: Record<string, unknown>) {
  const f: Record<string, unknown> = {};
  for (const field of cfg.fields) {
    if (row) {
      f[field.name] = row[field.name] ?? field.defaultValue ?? "";
    } else {
      f[field.name] = field.defaultValue ?? (field.type === "switch" ? false : "");
    }
  }
  return f;
}

export function ResourcePage({ cfg }: { cfg: ResourceConfig }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const idField = cfg.idField ?? "id";
  const adminConfig = usePublicAdminConfig();
  const displayTimeZone = adminConfig.data?.timezone?.trim() || undefined;

  const [page, setPage] = useState(1);
  const [filters, setFilters] = useState<Record<string, string>>({});

  const queryKey = useMemo(
    () => [cfg.name, page, filters],
    [cfg.name, page, filters],
  );
  const { data, error, isLoading } = useQuery({
    queryKey,
    queryFn: () =>
      apiGet<ListResult>(`${cfg.api}/list`, {
        page,
        page_size: PAGE_SIZE,
        ...Object.fromEntries(
          Object.entries(filters).filter(([, v]) => v !== ""),
        ),
      }),
  });

  const [open, setOpen] = useState(false);
  const [editingId, setEditingId] = useState<unknown>(null);
  const [form, setForm] = useState<Record<string, unknown>>({});
  const [deleteTarget, setDeleteTarget] = useState<Record<string, unknown> | null>(
    null,
  );
  const editing = editingId !== null;

  const openCreate = () => {
    setEditingId(null);
    setForm(initialForm(cfg));
    setOpen(true);
  };
  const openEdit = (row: Record<string, unknown>) => {
    setEditingId(row[idField]);
    setForm({ ...initialForm(cfg, row), [idField]: row[idField] });
    setOpen(true);
  };

  const save = useMutation({
    mutationFn: async () => {
      const url = editing ? `${cfg.api}/update` : `${cfg.api}/create`;
      const payload = editing ? { ...form, [idField]: editingId } : form;
      await apiPost(url, payload);
    },
    onSuccess: () => {
      setOpen(false);
      void qc.invalidateQueries({ queryKey: [cfg.name] });
    },
  });

  const remove = useMutation({
    mutationFn: (id: unknown) =>
      apiPost(`${cfg.api}/delete`, { [idField]: id }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: [cfg.name] }),
  });

  const rows = data?.list ?? [];
  const total = data?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const showActions = cfg.canEdit !== false || cfg.canDelete !== false;

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">{t(cfg.titleKey)}</h1>
        <div className="flex flex-wrap items-center gap-2">
          {(cfg.filters ?? []).map((flt) => (
            <Input
              aria-label={t(flt.label)}
              key={flt.name}
              placeholder={t(flt.label)}
              value={filters[flt.name] ?? ""}
              onChange={(e) => {
                setFilters((s) => ({ ...s, [flt.name]: e.target.value }));
                setPage(1);
              }}
            />
          ))}
          {cfg.canCreate !== false && (
            <Button onClick={openCreate}>{t("create")}</Button>
          )}
        </div>
      </div>

      <div className="overflow-x-auto rounded-lg border border-kumo-line">
        <Table>
          <Table.Header>
            <Table.Row>
              {cfg.columns.map((c) => (
                <Table.Head key={c.key}>{t(c.label)}</Table.Head>
              ))}
              {showActions && <Table.Head>{t("actions")}</Table.Head>}
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {rows.map((row, i) => (
              <Table.Row key={String(row[idField] ?? i)}>
                {cfg.columns.map((c) => (
                  <Table.Cell key={c.key}>
                    {c.render
                      ? c.render(row, t)
                      : asText(row[c.key], c.key, displayTimeZone)}
                  </Table.Cell>
                ))}
                {showActions && (
                  <Table.Cell>
                    <div className="flex gap-1">
                      {cfg.canEdit !== false && (
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => openEdit(row)}
                        >
                          {t("edit")}
                        </Button>
                      )}
                      {cfg.canDelete !== false && (
                        <Button
                          size="sm"
                          variant="secondary-destructive"
                          onClick={() => {
                            remove.reset();
                            setDeleteTarget(row);
                          }}
                        >
                          {t("delete")}
                        </Button>
                      )}
                    </div>
                  </Table.Cell>
                )}
              </Table.Row>
            ))}
          </Table.Body>
        </Table>
        {isLoading && (
          <TableState tone="loading">{t("loading")}</TableState>
        )}
        {error && (
          <TableState tone="error">
            {(error as Error).message || t("operationFailed")}
          </TableState>
        )}
        {!isLoading && !error && rows.length === 0 && (
          <TableState tone="empty">{t("noData")}</TableState>
        )}
      </div>

      <div className="mt-4 flex items-center justify-end gap-3 text-sm">
        <span>
          {page} / {totalPages} · {total}
        </span>
        <Button
          size="sm"
          variant="secondary"
          disabled={page <= 1}
          onClick={() => setPage((p) => p - 1)}
        >
          ‹
        </Button>
        <Button
          size="sm"
          variant="secondary"
          disabled={page >= totalPages}
          onClick={() => setPage((p) => p + 1)}
        >
          ›
        </Button>
      </div>

      {(cfg.canCreate !== false || cfg.canEdit !== false) && (
        <Dialog.Root open={open} onOpenChange={setOpen}>
          <Dialog size="lg" className={dialogPanelClass}>
            <DialogHeader
              title={`${editing ? t("edit") : t("create")} · ${t(cfg.titleKey)}`}
              description={editing ? t("editDialogHint") : t("createDialogHint")}
            />
            <DialogBody>
              <div className="grid gap-4">
                {cfg.fields
                  .filter((f) => !(editing && f.createOnly))
                  .map((field) => (
                    <FieldInput
                      key={field.name}
                      field={field}
                      editing={editing}
                      value={form[field.name]}
                      onChange={(v) =>
                        setForm((s) => ({ ...s, [field.name]: v }))
                      }
                    />
                  ))}
              </div>
            </DialogBody>
            <DialogFooter
              error={
                save.error
                  ? (save.error as Error).message || t("operationFailed")
                  : undefined
              }
            >
              <Button variant="secondary" onClick={() => setOpen(false)}>
                {t("cancel")}
              </Button>
              <Button onClick={() => save.mutate()} loading={save.isPending}>
                {t("save")}
              </Button>
            </DialogFooter>
          </Dialog>
        </Dialog.Root>
      )}

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("confirmDeleteTitle")}
        description={t("confirmDeleteDescription")}
        confirmLabel={t("delete")}
        cancelLabel={t("cancel")}
        error={
          remove.error
            ? (remove.error as Error).message || t("operationFailed")
            : undefined
        }
        loading={remove.isPending}
        onOpenChange={(next) => {
          if (!next) {
            setDeleteTarget(null);
            remove.reset();
          }
        }}
        onConfirm={() => {
          if (!deleteTarget) return;
          remove.mutate(deleteTarget[idField], {
            onSuccess: () => setDeleteTarget(null),
          });
        }}
      />
    </div>
  );
}

function asText(v: unknown, key = "", timeZone?: string): string {
  if (v === null || v === undefined) return "";
  if (isDateTimeKey(key)) {
    return formatUtcDateTime(v, timeZone);
  }
  if (key === "expired_at") {
    return formatUnixSeconds(v, timeZone);
  }
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function isDateTimeKey(key: string) {
  return key === "created_at" || key === "updated_at";
}

function formatUtcDateTime(value: unknown, timeZone?: string) {
  if (typeof value !== "string" || value.trim() === "") return asText(value);
  const iso = value.includes("T")
    ? value
    : value.trim().replace(" ", "T") + "Z";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return value;
  return formatDate(date, timeZone);
}

function formatUnixSeconds(value: unknown, timeZone?: string) {
  const seconds =
    typeof value === "number"
      ? value
      : typeof value === "string"
        ? Number(value)
        : Number.NaN;
  if (!Number.isFinite(seconds) || seconds <= 0) return asText(value);
  return formatDate(new Date(seconds * 1000), timeZone);
}

function formatDate(date: Date, timeZone?: string) {
  const options: Intl.DateTimeFormatOptions = {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
    ...(timeZone ? { timeZone } : {}),
  };
  try {
    return formatDateParts(date, options);
  } catch {
    const { timeZone: _timeZone, ...fallback } = options;
    return formatDateParts(date, fallback);
  }
}

function formatDateParts(date: Date, options: Intl.DateTimeFormatOptions) {
  const parts = new Intl.DateTimeFormat("en-US", options).formatToParts(date);
  const get = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((part) => part.type === type)?.value ?? "";
  return `${get("year")}-${get("month")}-${get("day")} ${get("hour")}:${get("minute")}:${get("second")}`;
}

function FieldInput({
  field,
  editing,
  value,
  onChange,
}: {
  field: FieldDef;
  editing: boolean;
  value: unknown;
  onChange: (v: unknown) => void;
}) {
  const { t } = useTranslation();
  const locked = editing && field.lockOnEdit;

  if (field.type === "switch") {
    const on = field.switchOn ?? true;
    const off = field.switchOff ?? false;
    const checked = value === on || value === true;
    return (
      <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-2">
        <Switch
          label={t(field.label)}
          controlFirst={false}
          checked={checked}
          disabled={locked}
          onCheckedChange={(v: boolean) => onChange(v ? on : off)}
        />
      </div>
    );
  }

  if (field.type === "select") {
    return (
      <label className="block">
        <span className="mb-1 block text-sm">{t(field.label)}</span>
        <select
          className="h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
          value={String(value ?? "")}
          disabled={locked}
          onChange={(e) => {
            const raw = e.target.value;
            const opt = field.options?.find((o) => String(o.value) === raw);
            onChange(opt ? opt.value : raw);
          }}
        >
          {(field.options ?? []).map((o) => (
            <option key={String(o.value)} value={String(o.value)}>
              {t(o.label)}
            </option>
          ))}
        </select>
      </label>
    );
  }

  if (field.type === "textarea") {
    return (
      <label className="block">
        <span className="mb-1 block text-sm">{t(field.label)}</span>
        <textarea
          className="min-h-24 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
          value={value === null || value === undefined ? "" : String(value)}
          disabled={locked}
          onChange={(e) => onChange(e.target.value)}
        />
      </label>
    );
  }

  const inputType =
    field.type === "password"
      ? "password"
      : field.type === "number"
        ? "number"
        : "text";
  return (
    <label className="block">
      <span className="mb-1 block text-sm">{t(field.label)}</span>
      <Input
        aria-label={t(field.label)}
        type={inputType}
        value={value === null || value === undefined ? "" : String(value)}
        disabled={locked}
        onChange={(e) =>
          onChange(
            field.type === "number"
              ? Number(e.target.value) || 0
              : e.target.value,
          )
        }
      />
    </label>
  );
}
