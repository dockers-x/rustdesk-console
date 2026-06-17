import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Switch } from "@cloudflare/kumo/components/switch";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { InlineMessage } from "../components/InlineMessage";
import { OAUTH_PROVIDER_OPTIONS } from "../components/OAuthProviderBadge";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  resourceFormDialogPanelClass,
} from "../components/DialogLayout";
import { TableState } from "../components/TableState";
import { usePublicAdminConfig } from "../lib/adminTitle";
import { apiGet, apiPost, http } from "../lib/api";
import {
  formatDateTime,
  formatUnixSeconds,
} from "../lib/dateFormat";
import {
  parseStrategyOptionsValue,
  StrategyOptionsInput,
} from "./strategyOptions";
import type { FieldDef, ResourceConfig } from "./types";

interface ListResult {
  list: Record<string, unknown>[];
  total: number;
  page: number;
  page_size: number;
}

const PAGE_SIZE = 10;
const RELATION_PAGE_SIZE = 200;

const TAG_COLORS = [
  { name: "red", hex: "#f44336", value: 0xfff44336 },
  { name: "green", hex: "#4caf50", value: 0xff4caf50 },
  { name: "blue", hex: "#2196f3", value: 0xff2196f3 },
  { name: "orange", hex: "#ff9800", value: 0xffff9800 },
  { name: "purple", hex: "#9c27b0", value: 0xff9c27b0 },
  { name: "grey", hex: "#9e9e9e", value: 0xff9e9e9e },
  { name: "cyan", hex: "#00bcd4", value: 0xff00bcd4 },
  { name: "lime", hex: "#cddc39", value: 0xffcddc39 },
  { name: "teal", hex: "#009688", value: 0xff009688 },
  { name: "pink", hex: "#f48fb1", value: 0xfff48fb1 },
  { name: "indigo", hex: "#3f51b5", value: 0xff3f51b5 },
  { name: "brown", hex: "#795548", value: 0xff795548 },
  { name: "yellow", hex: "#ffff00", value: 0xffffff00 },
] as const;

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

function tagColorFromName(name: string) {
  const lower = name.trim().toLowerCase();
  const named = TAG_COLORS.find((c) => c.name === lower);
  if (named) return named.value;
  const palette = TAG_COLORS.filter((c) => c.name !== "yellow");
  const sum = Array.from(name).reduce((acc, ch) => acc + ch.charCodeAt(0), 0);
  return palette[sum % palette.length]?.value ?? TAG_COLORS[0].value;
}

function argbToHex(value: unknown) {
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) return "";
  return `#${((n >>> 0) & 0x00ffffff).toString(16).padStart(6, "0")}`;
}

function hexToArgb(hex: string) {
  const normalized = hex.trim().replace(/^#/, "");
  if (!/^[0-9a-fA-F]{6}$/.test(normalized)) return 0;
  return 0xff000000 + Number.parseInt(normalized, 16);
}

function normalizeFormPayload(cfg: ResourceConfig, source: Record<string, unknown>) {
  const payload = { ...source };
  for (const field of cfg.fields) {
    if (field.type === "color") {
      const current = Number(payload[field.name] ?? 0);
      if (!Number.isFinite(current) || current <= 0) {
        const seed = String(
          payload[field.colorSeedField ?? "name"] ?? "",
        ).trim();
        payload[field.name] = tagColorFromName(seed || "tag");
      }
    }
    if (field.type === "strategy_options") {
      payload[field.name] = parseStrategyOptionsValue(payload[field.name]);
    }
  }
  return payload;
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
  const [oneTimeSecret, setOneTimeSecret] = useState("");
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
      const rawPayload = editing ? { ...form, [idField]: editingId } : form;
      const payload = normalizeFormPayload(cfg, rawPayload);
      return await apiPost<Record<string, unknown>>(url, payload);
    },
    onSuccess: (data) => {
      const token = data && typeof data.token === "string" ? data.token : "";
      setOneTimeSecret(token);
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
  const showActions =
    cfg.rowActions || cfg.canEdit !== false || cfg.canDelete !== false;

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

      {oneTimeSecret && (
        <div className="mb-4">
          <InlineMessage tone="success">
            <span className="mr-2 font-medium">{t("oneTimeToken")}</span>
            <code className="break-all rounded border border-kumo-line bg-kumo-elevated px-1.5 py-0.5 font-mono text-xs">
              {oneTimeSecret}
            </code>
            <Button
              className="ml-2"
              size="sm"
              variant="secondary"
              onClick={() => void navigator.clipboard.writeText(oneTimeSecret)}
            >
              {t("copy")}
            </Button>
          </InlineMessage>
        </div>
      )}

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
                    <div className="flex flex-wrap items-start gap-1">
                      {cfg.rowActions?.(row, t, {
                        openDelete: (target) => {
                          remove.reset();
                          setDeleteTarget(target);
                        },
                      })}
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
        <>
          {open && (
            <div
              aria-hidden="true"
              className="pointer-events-none fixed inset-0 z-40 bg-kumo-recessed opacity-95"
            />
          )}
          <Dialog.Root open={open} onOpenChange={setOpen}>
            <Dialog size="lg" className={resourceFormDialogPanelClass}>
              <DialogHeader
                title={`${editing ? t("edit") : t("create")} · ${t(cfg.titleKey)}`}
                description={editing ? t("editDialogHint") : t("createDialogHint")}
              />
              <DialogBody>
                <div className="grid gap-4 sm:grid-cols-2">
                  {cfg.fields
                    .filter((f) => !(editing && f.createOnly))
                    .filter((f) => !f.visibleWhen || f.visibleWhen(form, editing))
                    .map((field) => {
                      const fullWidth =
                        field.type === "textarea" ||
                        field.type === "switch" ||
                        field.type === "color" ||
                        field.type === "avatar" ||
                        field.type === "strategy_options" ||
                        field.type === "oauth_provider";
                      return (
                        <div
                          key={field.name}
                          className={fullWidth ? "sm:col-span-2" : undefined}
                        >
                          <FieldInput
                            field={field}
                            editing={editing}
                            value={form[field.name]}
                            form={form}
                            onChange={(v) =>
                              setForm((s) => {
                                const next = { ...s, [field.name]: v };
                                for (const resetName of field.resetFieldsOnChange ?? []) {
                                  const resetField = cfg.fields.find(
                                    (candidate) => candidate.name === resetName,
                                  );
                                  next[resetName] =
                                    resetField?.defaultValue ??
                                    (resetField?.type === "switch" ? false : "");
                                }
                                return next;
                              })
                            }
                          />
                        </div>
                      );
                    })}
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
        </>
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
    return formatDateTime(v, timeZone);
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

function FieldInput({
  field,
  editing,
  value,
  form,
  onChange,
}: {
  field: FieldDef;
  editing: boolean;
  value: unknown;
  form: Record<string, unknown>;
  onChange: (v: unknown) => void;
}) {
  const { t } = useTranslation();
  const locked = Boolean(editing && field.lockOnEdit);

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
        {field.hint && (
          <span className="mt-1 block text-xs text-kumo-subtle">
            {t(field.hint)}
          </span>
        )}
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
        {field.hint && (
          <span className="mt-1 block text-xs text-kumo-subtle">
            {t(field.hint)}
          </span>
        )}
      </label>
    );
  }

  if (field.type === "oauth_provider") {
    return (
      <OAuthProviderInput
        field={field}
        value={value}
        locked={locked}
        onChange={onChange}
      />
    );
  }

  if (field.type === "relation") {
    return (
      <RelationInput
        field={field}
        value={value}
        form={form}
        locked={locked}
        onChange={onChange}
      />
    );
  }

  if (field.type === "color") {
    return (
      <TagColorInput
        field={field}
        value={value}
        form={form}
        locked={locked}
        onChange={onChange}
      />
    );
  }

  if (field.type === "avatar") {
    return (
      <AvatarInput
        field={field}
        value={value}
        locked={locked}
        onChange={onChange}
      />
    );
  }

  if (field.type === "strategy_options") {
    return (
      <StrategyOptionsInput
        label={t(field.label)}
        hint={field.hint ? t(field.hint) : undefined}
        value={value}
        onChange={onChange as (v: Record<string, string>) => void}
      />
    );
  }

  if (field.type === "textarea") {
    return (
      <label className="block">
        <span className="mb-1 block text-sm">{t(field.label)}</span>
        <textarea
          className="min-h-24 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
          value={value === null || value === undefined ? "" : String(value)}
          placeholder={field.placeholder ? t(field.placeholder) : undefined}
          disabled={locked}
          onChange={(e) => onChange(e.target.value)}
        />
        {field.hint && (
          <span className="mt-1 block text-xs text-kumo-subtle">
            {t(field.hint)}
          </span>
        )}
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
        className="w-full"
        type={inputType}
        value={value === null || value === undefined ? "" : String(value)}
        placeholder={field.placeholder ? t(field.placeholder) : undefined}
        disabled={locked}
        onChange={(e) =>
          onChange(
            field.type === "number"
              ? Number(e.target.value) || 0
              : e.target.value,
          )
        }
      />
      {field.hint && (
        <span className="mt-1 block text-xs text-kumo-subtle">
          {t(field.hint)}
        </span>
      )}
    </label>
  );
}

function RelationInput({
  field,
  value,
  form,
  locked,
  onChange,
}: {
  field: FieldDef;
  value: unknown;
  form: Record<string, unknown>;
  locked: boolean;
  onChange: (v: unknown) => void;
}) {
  const { t } = useTranslation();
  const relation = field.relation;
  const api = typeof relation?.api === "function" ? relation.api(form) : relation?.api;
  const params =
    typeof relation?.params === "function"
      ? relation.params(form)
      : relation?.params ?? {};
  const valueField = relation?.valueField ?? "id";
  const labelFields = relation?.labelFields ?? ["name", "username", "hostname", "id"];

  const { data, isLoading, error } = useQuery({
    queryKey: ["relation", field.name, api, params],
    enabled: Boolean(api),
    queryFn: () =>
      apiGet<ListResult>(`${api}/list`, {
        page: 1,
        page_size: RELATION_PAGE_SIZE,
        ...params,
      }),
  });

  const rows = data?.list ?? [];
  const includeEmpty = relation?.includeEmptyOption ?? true;
  const selectedValue =
    value === null || value === undefined || Number(value) === 0
      ? ""
      : String(value);

  return (
    <label className="block">
      <span className="mb-1 block text-sm">{t(field.label)}</span>
      <select
        className="h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
        value={selectedValue}
        disabled={locked || isLoading || Boolean(error)}
        onChange={(e) => {
          const raw = e.target.value;
          if (relation?.valueAsString) {
            onChange(raw);
          } else {
            onChange(raw === "" ? 0 : Number(raw) || raw);
          }
        }}
      >
        {(includeEmpty || selectedValue === "") && (
          <option value="">
            {t(relation?.emptyLabel ?? "selectResource")}
          </option>
        )}
        {rows.map((row) => {
          const rawValue = row[valueField];
          const label = relationLabel(row, labelFields);
          return (
            <option key={String(rawValue)} value={String(rawValue)}>
              {label}
            </option>
          );
        })}
      </select>
      <span className="mt-1 block text-xs text-kumo-subtle">
        {error
          ? (error as Error).message || t("operationFailed")
          : isLoading
            ? t("loading")
            : field.hint
              ? t(field.hint)
              : t("selectResourceHint")}
      </span>
    </label>
  );
}

function OAuthProviderInput({
  field,
  value,
  locked,
  onChange,
}: {
  field: FieldDef;
  value: unknown;
  locked: boolean;
  onChange: (v: unknown) => void;
}) {
  const { t } = useTranslation();
  const selected = String(value ?? "");

  return (
    <div className="rounded-lg border border-kumo-line bg-kumo-base p-3">
      <span className="mb-2 block text-sm">{t(field.label)}</span>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {OAUTH_PROVIDER_OPTIONS.map((provider) => {
          const Icon = provider.icon;
          return (
            <button
              key={provider.value}
              type="button"
              disabled={locked}
              className={[
                "flex min-h-[44px] min-w-0 items-center justify-center gap-2 rounded-lg border px-3 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand disabled:opacity-50",
                selected === provider.value
                  ? "border-kumo-brand bg-kumo-tint text-kumo-default"
                  : "border-kumo-line bg-kumo-elevated text-kumo-subtle hover:text-kumo-default",
              ].join(" ")}
              onClick={() => onChange(provider.value)}
            >
              {provider.image ? (
                <img
                  src={provider.image}
                  alt=""
                  className="size-4 shrink-0 rounded-full"
                  aria-hidden="true"
                />
              ) : Icon ? (
                <Icon
                  size={15}
                  weight="regular"
                  className="shrink-0"
                  aria-hidden
                />
              ) : null}
              <span className="whitespace-nowrap">{provider.label}</span>
            </button>
          );
        })}
      </div>
      {field.hint && (
        <span className="mt-2 block text-xs text-kumo-subtle">
          {t(field.hint)}
        </span>
      )}
    </div>
  );
}

function relationLabel(row: Record<string, unknown>, fields: string[]) {
  const main =
    fields
      .filter((f) => f !== "id")
      .map((f) => row[f])
      .find((v) => v !== null && v !== undefined && String(v).trim() !== "") ??
    row.id;
  const id = row.id;
  if (id !== null && id !== undefined && String(id) !== String(main)) {
    return `${String(main)} (#${String(id)})`;
  }
  return String(main ?? "");
}

function TagColorInput({
  field,
  value,
  form,
  locked,
  onChange,
}: {
  field: FieldDef;
  value: unknown;
  form: Record<string, unknown>;
  locked: boolean;
  onChange: (v: unknown) => void;
}) {
  const { t } = useTranslation();
  const seed = String(form[field.colorSeedField ?? "name"] ?? "");
  const colorValue = Number(value) > 0 ? Number(value) : tagColorFromName(seed || "tag");
  const hex = argbToHex(colorValue) || "#f44336";

  return (
    <div className="rounded-lg border border-kumo-line bg-kumo-base p-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <span className="text-sm">{t(field.label)}</span>
        <label className="inline-flex items-center gap-2 text-xs text-kumo-subtle">
          <span>{hex}</span>
          <input
            aria-label={t(field.label)}
            type="color"
            className="size-8 rounded border border-kumo-line bg-transparent p-0"
            value={hex}
            disabled={locked}
            onChange={(e) => onChange(hexToArgb(e.target.value))}
          />
        </label>
      </div>
      <div className="grid grid-cols-7 gap-2 sm:grid-cols-[repeat(13,minmax(0,1fr))]">
        {TAG_COLORS.map((c) => (
          <button
            key={c.name}
            type="button"
            className="flex size-8 items-center justify-center rounded-md border border-kumo-line focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand disabled:opacity-50"
            style={{ backgroundColor: c.hex }}
            aria-label={`${t(field.label)} ${c.name}`}
            disabled={locked}
            onClick={() => onChange(c.value)}
          >
            {c.value === colorValue && (
              <span className="size-3 rounded-full bg-kumo-elevated shadow-sm" />
            )}
          </button>
        ))}
      </div>
      {field.hint && (
        <span className="mt-2 block text-xs text-kumo-subtle">
          {t(field.hint)}
        </span>
      )}
    </div>
  );
}

function AvatarInput({
  field,
  value,
  locked,
  onChange,
}: {
  field: FieldDef;
  value: unknown;
  locked: boolean;
  onChange: (v: unknown) => void;
}) {
  const { t } = useTranslation();
  const avatar = value === null || value === undefined ? "" : String(value);
  const upload = useMutation({
    mutationFn: async (file: File) => {
      const formData = new FormData();
      formData.append("file", file);
      return (await http.post("/api/admin/file/upload", formData)) as unknown as {
        url?: string;
      };
    },
    onSuccess: (data) => {
      if (data.url) onChange(data.url);
    },
  });

  const previewable =
    avatar.startsWith("http://") ||
    avatar.startsWith("https://") ||
    avatar.startsWith("data:image/") ||
    avatar.startsWith("/");

  return (
    <div className="rounded-lg border border-kumo-line bg-kumo-base p-3">
      <span className="mb-2 block text-sm">{t(field.label)}</span>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <div className="flex size-16 shrink-0 items-center justify-center overflow-hidden rounded-full border border-kumo-line bg-kumo-elevated text-xs text-kumo-subtle">
          {previewable ? (
            <img
              src={avatar}
              alt={t("avatarPreview")}
              className="size-full object-cover"
            />
          ) : (
            t("avatar")
          )}
        </div>
        <div className="min-w-0 flex-1">
          <Input
            aria-label={t(field.label)}
            className="w-full"
            value={avatar}
            placeholder={field.placeholder ? t(field.placeholder) : undefined}
            disabled={locked}
            onChange={(e) => onChange(e.target.value)}
          />
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <label className="inline-flex min-h-9 cursor-pointer items-center rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm transition-colors hover:bg-kumo-tint">
              {upload.isPending ? t("uploading") : t("uploadAvatar")}
              <input
                type="file"
                accept="image/*"
                className="sr-only"
                disabled={locked || upload.isPending}
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) upload.mutate(file);
                  e.currentTarget.value = "";
                }}
              />
            </label>
            <Button
              size="sm"
              variant="secondary"
              disabled={locked || !avatar}
              onClick={() => onChange("")}
            >
              {t("clear")}
            </Button>
          </div>
          <span className="mt-1 block text-xs text-kumo-subtle">
            {upload.error
              ? (upload.error as Error).message || t("operationFailed")
              : field.hint
                ? t(field.hint)
                : t("avatarHint")}
          </span>
        </div>
      </div>
    </div>
  );
}
