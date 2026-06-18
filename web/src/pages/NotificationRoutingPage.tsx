import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, type BadgeVariant } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPost } from "../lib/api";

interface ListResult<T> {
  list: T[];
  total: number;
  page: number;
  page_size: number;
}

interface WebhookSubscription {
  id: number;
  name: string;
  url: string;
  event_types: string[];
  device_ids: string[];
  device_group_ids: number[];
  enabled: boolean;
  secret_set: boolean;
}

interface WebhookDelivery {
  id: number;
  subscription_id: number;
  event_id: string;
  event_type: string;
  status: string;
  status_code: number;
  attempt: number;
  error: string;
  delivered_at: number;
}

interface DeviceGroup {
  id: number;
  name: string;
}

interface FormState {
  id?: number;
  name: string;
  url: string;
  secret: string;
  clear_secret: boolean;
  event_types: string[];
  device_ids_text: string;
  device_group_ids: number[];
  enabled: boolean;
}

const EVENT_TYPES = ["device.online", "device.offline"];

const emptyForm: FormState = {
  name: "",
  url: "",
  secret: "",
  clear_secret: false,
  event_types: [...EVENT_TYPES],
  device_ids_text: "",
  device_group_ids: [],
  enabled: true,
};

function splitDeviceIds(value: string) {
  return value
    .split(/[,\n\r\s]+/)
    .map((item) => item.trim())
    .filter(Boolean)
    .filter((item, index, list) => list.indexOf(item) === index);
}

function statusTone(status: string): BadgeVariant {
  if (status === "success") return "success";
  if (status === "failed") return "error";
  return "secondary";
}

export function NotificationRoutingPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [formOpen, setFormOpen] = useState(false);
  const [form, setForm] = useState<FormState>(emptyForm);
  const [deleteTarget, setDeleteTarget] = useState<WebhookSubscription | null>(
    null,
  );
  const [message, setMessage] = useState("");

  const subscriptions = useQuery({
    queryKey: ["webhook-subscriptions"],
    queryFn: () =>
      apiGet<ListResult<WebhookSubscription>>(
        "/api/admin/webhook/subscriptions",
        { page: 1, page_size: 100 },
      ),
  });

  const deliveries = useQuery({
    queryKey: ["webhook-deliveries"],
    queryFn: () =>
      apiGet<ListResult<WebhookDelivery>>("/api/admin/webhook/deliveries", {
        page: 1,
        page_size: 20,
      }),
  });

  const groups = useQuery({
    queryKey: ["device-groups-for-webhook"],
    queryFn: () =>
      apiGet<ListResult<DeviceGroup>>("/api/admin/device_group/list", {
        page: 1,
        page_size: 999,
      }),
  });

  const save = useMutation({
    mutationFn: () =>
      apiPost<WebhookSubscription>("/api/admin/webhook/subscriptions", {
        id: form.id ?? 0,
        name: form.name,
        url: form.url,
        secret: form.secret,
        clear_secret: form.clear_secret,
        event_types: form.event_types,
        device_ids: splitDeviceIds(form.device_ids_text),
        device_group_ids: form.device_group_ids,
        enabled: form.enabled,
      }),
    onSuccess: () => {
      setFormOpen(false);
      setMessage(t("webhookSaved"));
      void qc.invalidateQueries({ queryKey: ["webhook-subscriptions"] });
    },
  });

  const remove = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/webhook/subscription/delete", { id }),
    onSuccess: () => {
      setDeleteTarget(null);
      setMessage(t("webhookDeleted"));
      void qc.invalidateQueries({ queryKey: ["webhook-subscriptions"] });
    },
  });

  const test = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/webhook/subscription/test", { id }),
    onSuccess: () => {
      setMessage(t("webhookTestSent"));
      void qc.invalidateQueries({ queryKey: ["webhook-deliveries"] });
    },
  });

  const groupNameById = useMemo(() => {
    const map = new Map<number, string>();
    for (const group of groups.data?.list ?? []) map.set(group.id, group.name);
    return map;
  }, [groups.data]);

  const openCreate = () => {
    setForm(emptyForm);
    setMessage("");
    save.reset();
    setFormOpen(true);
  };

  const openEdit = (row: WebhookSubscription) => {
    setForm({
      id: row.id,
      name: row.name,
      url: row.url,
      secret: "",
      clear_secret: false,
      event_types: row.event_types.length ? row.event_types : [...EVENT_TYPES],
      device_ids_text: row.device_ids.join("\n"),
      device_group_ids: row.device_group_ids,
      enabled: row.enabled,
    });
    setMessage("");
    save.reset();
    setFormOpen(true);
  };

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold">{t("notificationRouting")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("notificationRoutingHint")}
          </p>
        </div>
        <Button onClick={openCreate}>{t("createWebhook")}</Button>
      </div>

      {message && (
        <InlineMessage tone="success" className="mb-4">
          {message}
        </InlineMessage>
      )}

      <section className="mb-5 rounded-lg border border-kumo-line bg-kumo-elevated">
        <div className="border-b border-kumo-line px-4 py-3">
          <h2 className="text-base font-semibold">{t("webhookSubscriptions")}</h2>
          <p className="mt-1 text-sm text-kumo-subtle">
            {t("webhookSubscriptionsHint")}
          </p>
        </div>
        <div className="overflow-x-auto">
          <Table>
            <Table.Header>
              <Table.Row>
                <Table.Head>{t("name")}</Table.Head>
                <Table.Head>{t("webhookUrl")}</Table.Head>
                <Table.Head>{t("events")}</Table.Head>
                <Table.Head>{t("filters")}</Table.Head>
                <Table.Head>{t("status")}</Table.Head>
                <Table.Head>{t("actions")}</Table.Head>
              </Table.Row>
            </Table.Header>
            <Table.Body>
              {(subscriptions.data?.list ?? []).map((row) => (
                <Table.Row key={row.id}>
                  <Table.Cell className="font-medium">{row.name}</Table.Cell>
                  <Table.Cell>
                    <span className="block max-w-[20rem] truncate font-mono text-xs">
                      {row.url}
                    </span>
                  </Table.Cell>
                  <Table.Cell>
                    <div className="flex flex-wrap gap-1">
                      {row.event_types.map((event) => (
                        <Badge key={event} variant="secondary">
                          {event}
                        </Badge>
                      ))}
                    </div>
                  </Table.Cell>
                  <Table.Cell>
                    <span className="text-xs text-kumo-subtle">
                      {row.device_ids.length === 0 &&
                      row.device_group_ids.length === 0
                        ? t("allDevices")
                        : [
                            row.device_ids.length
                              ? t("deviceFilterCount").replace(
                                  "{{count}}",
                                  String(row.device_ids.length),
                                )
                              : "",
                            row.device_group_ids.length
                              ? t("groupFilterCount").replace(
                                  "{{count}}",
                                  String(row.device_group_ids.length),
                                )
                              : "",
                          ]
                            .filter(Boolean)
                            .join(" · ")}
                    </span>
                  </Table.Cell>
                  <Table.Cell>
                    <Badge variant={row.enabled ? "success" : "secondary"}>
                      {row.enabled ? t("enabled") : t("disabled")}
                    </Badge>
                  </Table.Cell>
                  <Table.Cell>
                    <div className="flex flex-wrap gap-1">
                      <Button
                        size="sm"
                        variant="ghost"
                        loading={test.isPending}
                        onClick={() => test.mutate(row.id)}
                      >
                        {t("testWebhook")}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => openEdit(row)}
                      >
                        {t("edit")}
                      </Button>
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
                    </div>
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
          {subscriptions.isLoading && (
            <TableState tone="loading">{t("loading")}</TableState>
          )}
          {subscriptions.error && (
            <TableState tone="error">
              {(subscriptions.error as Error).message || t("operationFailed")}
            </TableState>
          )}
          {!subscriptions.isLoading &&
            !subscriptions.error &&
            (subscriptions.data?.list ?? []).length === 0 && (
              <TableState tone="empty">{t("webhookEmpty")}</TableState>
            )}
        </div>
      </section>

      <section className="rounded-lg border border-kumo-line bg-kumo-elevated">
        <div className="border-b border-kumo-line px-4 py-3">
          <h2 className="text-base font-semibold">{t("webhookDeliveries")}</h2>
          <p className="mt-1 text-sm text-kumo-subtle">
            {t("webhookDeliveriesHint")}
          </p>
        </div>
        <div className="overflow-x-auto">
          <Table>
            <Table.Header>
              <Table.Row>
                <Table.Head>{t("event")}</Table.Head>
                <Table.Head>{t("status")}</Table.Head>
                <Table.Head>{t("httpStatus")}</Table.Head>
                <Table.Head>{t("attempt")}</Table.Head>
                <Table.Head>{t("error")}</Table.Head>
              </Table.Row>
            </Table.Header>
            <Table.Body>
              {(deliveries.data?.list ?? []).map((row) => (
                <Table.Row key={row.id}>
                  <Table.Cell>
                    <div className="font-mono text-xs">{row.event_type}</div>
                    <div className="mt-1 max-w-[16rem] truncate text-xs text-kumo-subtle">
                      {row.event_id}
                    </div>
                  </Table.Cell>
                  <Table.Cell>
                    <Badge variant={statusTone(row.status)}>
                      {row.status}
                    </Badge>
                  </Table.Cell>
                  <Table.Cell>{row.status_code || "—"}</Table.Cell>
                  <Table.Cell>{row.attempt}</Table.Cell>
                  <Table.Cell>
                    <span className="block max-w-[24rem] truncate text-xs text-kumo-subtle">
                      {row.error || "—"}
                    </span>
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
          {deliveries.isLoading && (
            <TableState tone="loading">{t("loading")}</TableState>
          )}
          {deliveries.error && (
            <TableState tone="error">
              {(deliveries.error as Error).message || t("operationFailed")}
            </TableState>
          )}
          {!deliveries.isLoading &&
            !deliveries.error &&
            (deliveries.data?.list ?? []).length === 0 && (
              <TableState tone="empty">{t("webhookDeliveryEmpty")}</TableState>
            )}
        </div>
      </section>

      <Dialog.Root open={formOpen} onOpenChange={setFormOpen}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader
            title={form.id ? t("editWebhook") : t("createWebhook")}
            description={t("webhookDialogHint")}
          />
          <DialogBody>
            <div className="grid gap-4">
              <label className="block">
                <span className="mb-1 block text-sm">{t("name")}</span>
                <Input
                  value={form.name}
                  onChange={(e) =>
                    setForm((state) => ({ ...state, name: e.target.value }))
                  }
                />
              </label>
              <label className="block">
                <span className="mb-1 block text-sm">{t("webhookUrl")}</span>
                <Input
                  value={form.url}
                  placeholder="https://example.com/webhook"
                  onChange={(e) =>
                    setForm((state) => ({ ...state, url: e.target.value }))
                  }
                />
              </label>
              <label className="block">
                <span className="mb-1 block text-sm">{t("webhookSecret")}</span>
                <Input
                  value={form.secret}
                  placeholder={form.id ? t("keepExistingSecret") : ""}
                  onChange={(e) =>
                    setForm((state) => ({ ...state, secret: e.target.value }))
                  }
                />
              </label>
              {form.id && (
                <label className="inline-flex min-h-10 items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={form.clear_secret}
                    onChange={(e) =>
                      setForm((state) => ({
                        ...state,
                        clear_secret: e.target.checked,
                      }))
                    }
                  />
                  {t("clearWebhookSecret")}
                </label>
              )}
              <fieldset className="grid gap-2">
                <legend className="text-sm">{t("events")}</legend>
                <div className="grid gap-2 sm:grid-cols-2">
                  {EVENT_TYPES.map((event) => (
                    <label
                      key={event}
                      className="inline-flex min-h-10 items-center gap-2 rounded-lg border border-kumo-line bg-kumo-base px-3 text-sm"
                    >
                      <input
                        type="checkbox"
                        checked={form.event_types.includes(event)}
                        onChange={(e) =>
                          setForm((state) => ({
                            ...state,
                            event_types: e.target.checked
                              ? [...state.event_types, event]
                              : state.event_types.filter((item) => item !== event),
                          }))
                        }
                      />
                      {event}
                    </label>
                  ))}
                </div>
              </fieldset>
              <label className="block">
                <span className="mb-1 block text-sm">{t("deviceIdFilter")}</span>
                <textarea
                  className="min-h-24 w-full rounded-lg border border-kumo-line bg-kumo-base px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                  value={form.device_ids_text}
                  placeholder="123456789&#10;987654321"
                  onChange={(e) =>
                    setForm((state) => ({
                      ...state,
                      device_ids_text: e.target.value,
                    }))
                  }
                />
                <span className="mt-1 block text-xs text-kumo-subtle">
                  {t("deviceIdFilterHint")}
                </span>
              </label>
              <fieldset className="grid gap-2">
                <legend className="text-sm">{t("deviceGroupFilter")}</legend>
                <div className="grid max-h-40 gap-2 overflow-auto rounded-lg border border-kumo-line bg-kumo-base p-2 sm:grid-cols-2">
                  {(groups.data?.list ?? []).map((group) => (
                    <label
                      key={group.id}
                      className="inline-flex min-h-9 items-center gap-2 px-2 text-sm"
                    >
                      <input
                        type="checkbox"
                        checked={form.device_group_ids.includes(group.id)}
                        onChange={(e) =>
                          setForm((state) => ({
                            ...state,
                            device_group_ids: e.target.checked
                              ? [...state.device_group_ids, group.id]
                              : state.device_group_ids.filter(
                                  (item) => item !== group.id,
                                ),
                          }))
                        }
                      />
                      {groupNameById.get(group.id) ?? group.name}
                    </label>
                  ))}
                  {(groups.data?.list ?? []).length === 0 && (
                    <span className="px-2 py-1 text-sm text-kumo-subtle">
                      {t("noData")}
                    </span>
                  )}
                </div>
              </fieldset>
              <label className="inline-flex min-h-10 items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={form.enabled}
                  onChange={(e) =>
                    setForm((state) => ({ ...state, enabled: e.target.checked }))
                  }
                />
                {t("enabled")}
              </label>
            </div>
          </DialogBody>
          <DialogFooter
            error={
              save.error
                ? (save.error as Error).message || t("operationFailed")
                : undefined
            }
          >
            <Button variant="secondary" onClick={() => setFormOpen(false)}>
              {t("cancel")}
            </Button>
            <Button
              loading={save.isPending}
              disabled={
                save.isPending ||
                form.name.trim() === "" ||
                form.url.trim() === "" ||
                form.event_types.length === 0
              }
              onClick={() => save.mutate()}
            >
              {t("save")}
            </Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("confirmDeleteTitle")}
        description={t("confirmDeleteDescription")}
        confirmLabel={t("delete")}
        cancelLabel={t("cancel")}
        loading={remove.isPending}
        error={
          remove.error
            ? (remove.error as Error).message || t("operationFailed")
            : undefined
        }
        onOpenChange={(next) => {
          if (!next) setDeleteTarget(null);
        }}
        onConfirm={() => {
          if (deleteTarget) remove.mutate(deleteTarget.id);
        }}
      />
    </div>
  );
}
