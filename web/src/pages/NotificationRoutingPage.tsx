import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  useMutation,
  useMutationState,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { Badge, type BadgeVariant } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input, Textarea } from "@cloudflare/kumo/components/input";
import { Select } from "@cloudflare/kumo/components/select";
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
import {
  NotificationTemplatesPanel,
  useNotificationTemplates,
} from "./NotificationTemplatesPanel";

interface ListResult<T> {
  list: T[];
  total: number;
  page: number;
  page_size: number;
}

const CHANNELS = {
  webhook: "notificationChannelWebhook",
  ntfy: "notificationChannelNtfy",
  bark: "notificationChannelBark",
  server_chan: "notificationChannelServerChan",
} as const;

type Channel = keyof typeof CHANNELS;

const DEFAULT_URL: Record<Channel, string> = {
  webhook: "",
  ntfy: "https://ntfy.sh",
  bark: "https://api.day.app",
  server_chan: "",
};

interface NotificationSubscription {
  id: number;
  name: string;
  channel?: Channel;
  url: string;
  topic?: string;
  template_id?: number | null;
  event_types: string[];
  device_ids: string[];
  device_group_ids: number[];
  enabled: boolean;
  secret_set: boolean;
}

interface NotificationDelivery {
  id: number;
  subscription_id: number;
  subscription_name?: string;
  channel?: Channel;
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
  channel: Channel;
  url: string;
  stored_url: string;
  topic: string;
  template_id: number;
  secret: string;
  secret_set: boolean;
  clear_secret: boolean;
  event_types?: string[];
  device_ids_text: string;
  device_group_ids: number[];
  enabled: boolean;
}

const EVENT_TYPES = ["device.online", "device.offline"];
const ONLINE_EVENTS = ["device.online"];
const TEST_MUTATION_KEY = ["notification-test"];
const SECRET_LABEL = {
  webhook: "webhookSecret",
  ntfy: "notificationAccessToken",
  bark: "notificationDeviceKey",
  server_chan: "notificationSendKey",
} as const;

type TestRule = Pick<NotificationSubscription, "id" | "name">;

const emptyForm: FormState = {
  name: "",
  channel: "webhook",
  url: "",
  stored_url: "",
  topic: "",
  template_id: 0,
  secret: "",
  secret_set: false,
  clear_secret: false,
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
  const formRef = useRef<HTMLFormElement>(null);
  const pickerPortal = useRef<HTMLDivElement>(null);
  const testedDrafts = useRef(new WeakSet<FormState>());
  const [deleteTarget, setDeleteTarget] =
    useState<NotificationSubscription | null>(null);
  const [message, setMessage] = useState<{
    tone: "success" | "error";
    text: string;
  } | null>(null);

  const subscriptions = useQuery({
    queryKey: ["webhook-subscriptions"],
    queryFn: () =>
      apiGet<ListResult<NotificationSubscription>>(
        "/api/admin/webhook/subscriptions",
        { page: 1, page_size: 100 },
      ),
  });

  const deliveries = useQuery({
    queryKey: ["webhook-deliveries"],
    queryFn: () =>
      apiGet<ListResult<NotificationDelivery>>("/api/admin/webhook/deliveries", {
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

  const templates = useNotificationTemplates();
  const availableTemplates = templates.data?.list ?? [];
  const selectedTemplateAvailable = availableTemplates.some(
    (template) => template.id === form.template_id,
  );

  const eventTypes =
    form.event_types ??
    (form.channel === "webhook" ? EVENT_TYPES : ONLINE_EVENTS);
  const usesUrl = form.channel !== "server_chan";
  const usesTopic = form.channel === "ntfy";
  const requiresSecret = form.channel === "bark" || form.channel === "server_chan";
  const secretScopedToUrl = form.channel === "ntfy" || form.channel === "bark";
  const canKeepStoredSecret =
    form.secret_set &&
    (!secretScopedToUrl || form.url.trim() === form.stored_url);
  const clearingStoredSecret = canKeepStoredSecret && form.clear_secret;
  const configComplete =
    form.name.trim() !== "" &&
    (!usesUrl || form.url.trim() !== "") &&
    (!usesTopic || form.topic.trim() !== "") &&
    (!requiresSecret || canKeepStoredSecret || form.secret.trim() !== "") &&
    eventTypes.length > 0;

  const formConfig = () => ({
    id: form.id ?? 0,
    name: form.name,
    channel: form.channel,
    url: form.url,
    topic: form.topic,
    template_id: form.template_id,
    secret: form.secret,
    clear_secret: form.clear_secret && !form.secret.trim(),
    event_types: eventTypes,
    device_ids: splitDeviceIds(form.device_ids_text),
    device_group_ids: form.device_group_ids,
    enabled: form.enabled,
  });

  const closeForm = () => {
    setFormOpen(false);
    setForm(emptyForm);
    testedDrafts.current = new WeakSet<FormState>();
  };

  const save = useMutation({
    mutationFn: () =>
      apiPost<NotificationSubscription>(
        "/api/admin/webhook/subscriptions",
        formConfig(),
      ),
    onSuccess: () => {
      closeForm();
      setMessage({ tone: "success", text: t("notificationRuleSaved") });
      void qc.invalidateQueries({ queryKey: ["webhook-subscriptions"] });
    },
  });

  const remove = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/webhook/subscription/delete", { id }),
    onSuccess: () => {
      setDeleteTarget(null);
      setMessage({ tone: "success", text: t("notificationRuleDeleted") });
      void qc.invalidateQueries({ queryKey: ["webhook-subscriptions"] });
    },
  });

  const test = useMutation({
    mutationKey: TEST_MUTATION_KEY,
    mutationFn: ({ id }: TestRule) =>
      apiPost("/api/admin/webhook/subscription/test", { id }),
    onSuccess: (_, { name }) => {
      setMessage({ tone: "success", text: t("notificationTestSent", { name }) });
    },
    onError: (error, { name }) => {
      setMessage({
        tone: "error",
        text: t("notificationTestFailed", {
          name,
          error: error.message || t("operationFailed"),
        }),
      });
    },
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: ["webhook-deliveries"] });
    },
  });
  const pendingTestIds = useMutationState({
    filters: { mutationKey: TEST_MUTATION_KEY, status: "pending" },
    select: (mutation) => (mutation.state.variables as TestRule).id,
  });

  const draftTest = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/webhook/subscription/test", { config: formConfig() }),
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: ["webhook-deliveries"] });
    },
  });
  const formActionDisabled =
    !configComplete || save.isPending || draftTest.isPending;
  const showDraftResult = testedDrafts.current.has(form);

  const groupNameById = useMemo(() => {
    const map = new Map<number, string>();
    for (const group of groups.data?.list ?? []) map.set(group.id, group.name);
    return map;
  }, [groups.data]);

  const openCreate = () => {
    setForm(emptyForm);
    setMessage(null);
    save.reset();
    draftTest.reset();
    setFormOpen(true);
  };

  const openEdit = (row: NotificationSubscription) => {
    setForm({
      id: row.id,
      name: row.name,
      channel: row.channel ?? "webhook",
      url: row.url,
      stored_url: row.url,
      topic: row.topic ?? "",
      template_id: row.template_id ?? 0,
      secret: "",
      secret_set: row.secret_set,
      clear_secret: false,
      event_types: row.event_types,
      device_ids_text: row.device_ids.join("\n"),
      device_group_ids: row.device_group_ids,
      enabled: row.enabled,
    });
    setMessage(null);
    save.reset();
    draftTest.reset();
    setFormOpen(true);
  };

  const changeChannel = (channel: Channel) => {
    setForm((state) => ({
      ...state,
      channel,
      url: DEFAULT_URL[channel],
      topic: "",
      secret: "",
      secret_set: false,
      clear_secret: state.clear_secret || state.secret_set,
    }));
  };

  const eventLabel = (event: string) => {
    if (event === "device.online") return t("online");
    if (event === "device.offline") return t("offline");
    if (event === "webhook.test") return t("notificationTestDelivery");
    return event;
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
        <Button onClick={openCreate}>{t("createNotificationRule")}</Button>
      </div>

      {message && (
        <InlineMessage tone={message.tone} className="mb-4">
          {message.text}
        </InlineMessage>
      )}

      <section className="mb-5 rounded-lg border border-kumo-line bg-kumo-elevated">
        <div className="border-b border-kumo-line px-4 py-3">
          <h2 className="text-base font-semibold">{t("notificationRules")}</h2>
          <p className="mt-1 text-sm text-kumo-subtle">
            {t("notificationRulesHint")}
          </p>
        </div>
        <div className="overflow-x-auto">
          <Table>
            <Table.Header>
              <Table.Row>
                <Table.Head>{t("name")}</Table.Head>
                <Table.Head>{t("notificationDestination")}</Table.Head>
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
                    <div className="font-medium">
                      {t(CHANNELS[row.channel ?? "webhook"])}
                    </div>
                    {row.url && (
                      <div className="mt-1 max-w-[20rem] break-all font-mono text-xs text-kumo-subtle">
                        {row.url}
                      </div>
                    )}
                    {row.topic && (
                      <div className="max-w-[20rem] break-all text-xs text-kumo-subtle">
                        {t("notificationTopic")}: {row.topic}
                      </div>
                    )}
                    {!!row.template_id && (
                      <div className="mt-1 text-xs text-kumo-subtle">
                        {availableTemplates.find((template) => template.id === row.template_id)?.name ||
                          `#${row.template_id}`}
                      </div>
                    )}
                  </Table.Cell>
                  <Table.Cell>
                    <div className="flex flex-wrap gap-1">
                      {row.event_types.map((event) => (
                        <Badge key={event} variant="secondary">
                          {eventLabel(event)}
                        </Badge>
                      ))}
                    </div>
                  </Table.Cell>
                  <Table.Cell>
                    <span className="text-xs text-kumo-default/80">
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
                        aria-label={t("notificationTestRule", { name: row.name })}
                        loading={pendingTestIds.includes(row.id)}
                        onClick={() => test.mutate(row)}
                      >
                        {t("notificationSendTest")}
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
              <TableState tone="empty">{t("notificationRulesEmpty")}</TableState>
            )}
        </div>
      </section>

      <section className="rounded-lg border border-kumo-line bg-kumo-elevated">
        <div className="border-b border-kumo-line px-4 py-3">
          <h2 className="text-base font-semibold">{t("notificationDeliveries")}</h2>
          <p className="mt-1 text-sm text-kumo-subtle">
            {t("notificationDeliveriesHint")}
          </p>
        </div>
        <div className="overflow-x-auto">
          <Table>
            <Table.Header>
              <Table.Row>
                <Table.Head>{t("notificationRule")}</Table.Head>
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
                    <div className="font-medium">
                      {row.subscription_name ||
                        (row.subscription_id === 0 && t("notificationDraftTest")) ||
                        subscriptions.data?.list.find(
                          (subscription) => subscription.id === row.subscription_id,
                        )?.name ||
                        `#${row.subscription_id}`}
                    </div>
                    <div className="mt-1 text-xs text-kumo-subtle">
                      {t(CHANNELS[row.channel ?? "webhook"])}
                    </div>
                  </Table.Cell>
                  <Table.Cell>
                    <div className="text-sm">{eventLabel(row.event_type)}</div>
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
              <TableState tone="empty">{t("notificationDeliveriesEmpty")}</TableState>
            )}
        </div>
      </section>

      <NotificationTemplatesPanel />

      <Dialog.Root
        open={formOpen}
        onOpenChange={(open) => {
          if (!open && !save.isPending) closeForm();
        }}
      >
        <Dialog size="lg" className={dialogPanelClass} style={{ overflow: "visible" }}>
          <DialogHeader
            title={
              form.id ? t("editNotificationRule") : t("createNotificationRule")
            }
          />
          <DialogBody>
            <form
              id="notification-rule-form"
              ref={formRef}
              className="grid gap-3"
              onSubmit={(e) => {
                e.preventDefault();
                if (formActionDisabled) return;
                draftTest.reset();
                save.mutate();
              }}
            >
              <div className="grid gap-3 sm:grid-cols-2">
                <label className="block">
                  <span className="mb-1 block text-sm">{t("name")}</span>
                  <Input
                    className="w-full"
                    required
                    value={form.name}
                    onChange={(e) =>
                      setForm((state) => ({ ...state, name: e.target.value }))
                    }
                  />
                </label>
                <Select<Channel>
                  className="w-full"
                  container={pickerPortal}
                  label={t("notificationChannel")}
                  value={form.channel}
                  onValueChange={(channel) => {
                    if (channel) changeChannel(channel);
                  }}
                  items={(Object.keys(CHANNELS) as Channel[]).map((channel) => ({
                    value: channel,
                    label: t(CHANNELS[channel]),
                  }))}
                />
              </div>
              {usesUrl && (
                <div className={usesTopic ? "grid gap-3 sm:grid-cols-[2fr_1fr]" : ""}>
                  <label className="block">
                    <span className="mb-1 block text-sm">
                      {t(form.channel === "webhook" ? "webhookUrl" : "notificationBaseUrl")}
                    </span>
                    <Input
                      className="w-full"
                      required
                      type="url"
                      value={form.url}
                      placeholder={DEFAULT_URL[form.channel] || "https://example.com/webhook"}
                      onChange={(e) =>
                        setForm((state) => ({ ...state, url: e.target.value }))
                      }
                    />
                  </label>
                  {usesTopic && (
                    <label className="block">
                      <span className="mb-1 block text-sm">
                        {t("notificationTopic")}
                      </span>
                      <Input
                        className="w-full"
                        required
                        value={form.topic}
                        onChange={(e) =>
                          setForm((state) => ({ ...state, topic: e.target.value }))
                        }
                      />
                    </label>
                  )}
                </div>
              )}
              <div>
                <label htmlFor="notification-secret" className="mb-1 block text-sm">
                  {t(SECRET_LABEL[form.channel])}
                  {!requiresSecret && (
                    <span className="text-xs text-kumo-subtle">
                      {" "}{t("notificationOptional")}
                    </span>
                  )}
                </label>
                <Input
                  id="notification-secret"
                  className="w-full"
                  type="password"
                  autoComplete="new-password"
                  required={requiresSecret && !canKeepStoredSecret}
                  disabled={clearingStoredSecret}
                  value={form.secret}
                  aria-describedby={
                    canKeepStoredSecret && !clearingStoredSecret
                      ? "notification-keep-secret"
                      : undefined
                  }
                  onChange={(e) =>
                    setForm((state) => ({ ...state, secret: e.target.value }))
                  }
                />
                {canKeepStoredSecret && !clearingStoredSecret && (
                  <p id="notification-keep-secret" className="mt-1 text-xs text-kumo-default/80">
                    {t("keepExistingSecret")}
                  </p>
                )}
              </div>
              {form.channel === "server_chan" && (
                <div className="flex flex-wrap gap-x-4 gap-y-2 text-sm">
                  <a
                    className="rounded-sm text-kumo-default underline underline-offset-2 focus-visible:outline-2"
                    href="https://sc3.ft07.com/sendkey"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t("notificationServerChanAppLink")}
                  </a>
                  <a
                    className="rounded-sm text-kumo-default underline underline-offset-2 focus-visible:outline-2"
                    href="https://sct.ftqq.com/sendkey"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t("notificationServerChanWeChatLink")}
                  </a>
                </div>
              )}
              {canKeepStoredSecret && !requiresSecret && (
                <label className="inline-flex min-h-10 items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={form.clear_secret}
                    onChange={(e) =>
                      setForm((state) => ({
                        ...state,
                        clear_secret: e.target.checked,
                        secret: e.target.checked ? "" : state.secret,
                      }))
                    }
                  />
                  {t(form.channel === "webhook" ? "clearWebhookSecret" : "notificationClearAccessToken")}
                </label>
              )}
              {form.channel === "webhook" && (
                <p className="text-xs text-kumo-default/80">{t("webhookDialogHint")}</p>
              )}
              <Select<number>
                className="w-full"
                container={pickerPortal}
                label={t("notificationTemplate")}
                value={form.template_id}
                onValueChange={(templateId) => {
                  if (templateId === null) return;
                  setForm((state) => ({ ...state, template_id: templateId }));
                }}
                items={[
                  { value: 0, label: t("notificationTemplateBuiltIn") },
                  ...(form.template_id > 0 && !selectedTemplateAvailable
                    ? [{ value: form.template_id, label: `#${form.template_id}` }]
                    : []),
                  ...availableTemplates.map((template) => ({
                    value: template.id,
                    label: template.name,
                  })),
                ]}
                description={templates.error ? t("notificationTemplatesUnavailable") : undefined}
              />
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
                        checked={eventTypes.includes(event)}
                        onChange={(e) =>
                          setForm((state) => ({
                            ...state,
                            event_types: e.target.checked
                              ? [...eventTypes, event]
                              : eventTypes.filter((item) => item !== event),
                          }))
                        }
                      />
                      {eventLabel(event)}
                    </label>
                  ))}
                </div>
              </fieldset>
              <div>
                <label htmlFor="notification-device-ids" className="mb-1 block text-sm">
                  {t("deviceIdFilter")}
                </label>
                <Textarea
                  id="notification-device-ids"
                  aria-describedby="notification-device-ids-hint"
                  className="min-h-24 w-full"
                  value={form.device_ids_text}
                  placeholder="123456789&#10;987654321"
                  onChange={(e) =>
                    setForm((state) => ({
                      ...state,
                      device_ids_text: e.target.value,
                    }))
                  }
                />
                <p id="notification-device-ids-hint" className="mt-1 text-xs text-kumo-default/80">
                  {t("deviceIdFilterHint")}
                </p>
              </div>
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
            </form>
          </DialogBody>
          <div className="shrink-0 overflow-hidden rounded-b-xl">
            <DialogFooter
              error={
                save.error
                  ? save.error.message || t("operationFailed")
                  : showDraftResult && draftTest.error
                    ? t("notificationDraftTestFailed", {
                        error: draftTest.error.message || t("operationFailed"),
                      })
                    : undefined
              }
            >
              {showDraftResult && draftTest.isSuccess && (
                <InlineMessage
                  tone="success"
                  className="w-full motion-safe:transition-opacity motion-safe:duration-150 motion-safe:ease-(--ease-out) motion-safe:starting:opacity-0"
                >
                  {t("notificationDraftTestSent")}
                </InlineMessage>
              )}
              <Button variant="secondary" disabled={save.isPending} onClick={closeForm}>
                {t("cancel")}
              </Button>
              <Button
                variant="secondary"
                disabled={formActionDisabled}
                loading={draftTest.isPending}
                onClick={() => {
                  if (!formRef.current?.reportValidity()) return;
                  save.reset();
                  testedDrafts.current.add(form);
                  draftTest.mutate();
                }}
              >
                {t("notificationSendTest")}
              </Button>
              <Button
                type="submit"
                form="notification-rule-form"
                loading={save.isPending}
                disabled={formActionDisabled}
              >
                {t("save")}
              </Button>
            </DialogFooter>
          </div>
          <div ref={pickerPortal} className="contents" />
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
