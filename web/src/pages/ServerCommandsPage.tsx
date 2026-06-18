import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Info } from "@phosphor-icons/react";
import { Badge } from "@cloudflare/kumo/components/badge";
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
import { TableState } from "../components/TableState";
import { apiGet, apiPatch, apiPost, ApiError } from "../lib/api";

const ID_TARGET = "21115";
const RELAY_TARGET = "21117";

interface ListResult {
  list: ServerCommand[];
  total: number;
}

interface ServerCommand {
  id: number;
  cmd: string;
  alias: string;
  option: string;
  explain: string;
  target: string;
}

interface RustdeskStatus {
  targets: {
    target: string;
    label: string;
    port?: number | null;
    available: boolean;
    message: string;
  }[];
  relay_servers?: string | null;
  always_use_relay?: string | null;
}

interface RelayPoolView {
  servers: string[];
  value: string;
  persisted: boolean;
}

interface RelayPoolSaveResult {
  pool: RelayPoolView;
  response: string;
}

interface RelayServerCheck {
  server: string;
  ok: boolean;
  message: string;
}

interface CommandForm {
  id?: number;
  cmd: string;
  alias: string;
  option: string;
  explain: string;
  target: string;
}

const emptyForm: CommandForm = {
  cmd: "",
  alias: "",
  option: "",
  explain: "",
  target: ID_TARGET,
};

function targetLabel(target: string) {
  if (target === ID_TARGET) return "ID";
  if (target === RELAY_TARGET) return "RELAY";
  return target || "—";
}

function relayLines(value: string) {
  return value
    .split(/[,\n\r;]+/)
    .map((item) => item.trim())
    .filter(Boolean)
    .join("\n");
}

function parseAlwaysUseRelay(value?: string | null) {
  const normalized = (value ?? "").toLowerCase();
  return normalized.includes("true") || normalized.includes("y");
}

export function ServerCommandsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [formOpen, setFormOpen] = useState(false);
  const [sendOpen, setSendOpen] = useState(false);
  const [form, setForm] = useState<CommandForm>(emptyForm);
  const [sendForm, setSendForm] = useState<CommandForm>(emptyForm);
  const [deleteTarget, setDeleteTarget] = useState<ServerCommand | null>(null);
  const [sendResult, setSendResult] = useState("");
  const [sendError, setSendError] = useState("");
  const [rulesOpen, setRulesOpen] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [relayServers, setRelayServers] = useState("");
  const [relayChecks, setRelayChecks] = useState<RelayServerCheck[]>([]);
  const [alwaysUseRelay, setAlwaysUseRelay] = useState(false);
  const [controlMessage, setControlMessage] = useState("");
  const [controlError, setControlError] = useState("");
  const [status, setStatus] = useState<Record<string, boolean | null>>({
    [ID_TARGET]: null,
    [RELAY_TARGET]: null,
  });

  const list = useQuery({
    queryKey: ["server-commands"],
    queryFn: () =>
      apiGet<ListResult>("/api/admin/rustdesk/cmdList", {
        page: 1,
        page_size: 9999,
      }),
  });

  const structured = useQuery({
    queryKey: ["rustdesk-status"],
    queryFn: () => apiGet<RustdeskStatus>("/api/admin/rustdesk/status"),
  });

  const relayPool = useQuery({
    queryKey: ["rustdesk-relay-pool"],
    queryFn: () => apiGet<RelayPoolView>("/api/admin/rustdesk/relayPool"),
  });

  useEffect(() => {
    if (!structured.data) return;
    const next: Record<string, boolean | null> = {
      [ID_TARGET]: null,
      [RELAY_TARGET]: null,
    };
    for (const target of structured.data.targets) {
      next[target.target] = target.available;
    }
    setStatus(next);
    setAlwaysUseRelay(parseAlwaysUseRelay(structured.data.always_use_relay));
  }, [structured.data]);

  useEffect(() => {
    if (!relayPool.data) return;
    setRelayServers(relayPool.data.persisted ? relayPool.data.servers.join("\n") : "");
  }, [relayPool.data]);

  const save = useMutation({
    mutationFn: () =>
      apiPost(
        form.id
          ? "/api/admin/rustdesk/cmdUpdate"
          : "/api/admin/rustdesk/cmdCreate",
        form,
      ),
    onSuccess: () => {
      setFormOpen(false);
      void qc.invalidateQueries({ queryKey: ["server-commands"] });
    },
  });

  const remove = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/rustdesk/cmdDelete", { id }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["server-commands"] }),
  });

  const send = useMutation({
    mutationFn: () =>
      apiPost<string>("/api/admin/rustdesk/sendCmd", {
        cmd: sendForm.cmd,
        option: sendForm.option,
        target: sendForm.target,
      }),
    onSuccess: (res) => {
      setSendError("");
      setSendResult(res || t("emptyResult"));
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSendError(ae.message || t("operationFailed"));
    },
  });

  const updateRelayServers = useMutation({
    mutationFn: (value?: string) =>
      apiPatch<RelayPoolSaveResult>("/api/admin/rustdesk/relayServers", {
        value: value ?? relayServers,
      }),
    onSuccess: (res) => {
      setControlError("");
      setControlMessage(res.response || t("operationSuccess"));
      setRelayServers(res.pool.persisted ? res.pool.servers.join("\n") : "");
      setRelayChecks([]);
      void relayPool.refetch();
      void structured.refetch();
    },
    onError: (err) => {
      const ae = err as ApiError;
      setControlError(ae.message || t("operationFailed"));
    },
  });

  const checkRelayServers = useMutation({
    mutationFn: (value: string) =>
      apiPost<RelayServerCheck[]>("/api/admin/rustdesk/relayServers/check", {
        value,
      }),
    onSuccess: (res) => {
      setRelayChecks(res);
      setControlError("");
      setControlMessage(t("relayPoolCheckComplete"));
    },
    onError: (err) => {
      const ae = err as ApiError;
      setControlError(ae.message || t("operationFailed"));
    },
  });

  const updateAlwaysUseRelay = useMutation({
    mutationFn: () =>
      apiPatch<string>("/api/admin/rustdesk/alwaysUseRelay", {
        enabled: alwaysUseRelay,
      }),
    onSuccess: (res) => {
      setControlError("");
      setControlMessage(res || t("operationSuccess"));
      void structured.refetch();
    },
    onError: (err) => {
      const ae = err as ApiError;
      setControlError(ae.message || t("operationFailed"));
    },
  });

  const queryIpBlocker = useMutation({
    mutationFn: () => apiGet<string>("/api/admin/rustdesk/ipBlocker"),
    onSuccess: (res) => {
      setControlError("");
      setControlMessage(res || t("emptyResult"));
    },
    onError: (err) => {
      const ae = err as ApiError;
      setControlError(ae.message || t("operationFailed"));
    },
  });

  const openCreate = () => {
    setForm(emptyForm);
    setFormOpen(true);
  };

  const openEdit = (row: ServerCommand) => {
    setForm({ ...row });
    setFormOpen(true);
  };

  const openSend = (row: Pick<ServerCommand, "cmd" | "option" | "target">) => {
    setSendForm({
      cmd: row.cmd,
      alias: "",
      option: row.option,
      explain: row.option ? `${row.cmd} ${row.option}` : row.cmd,
      target: row.target,
    });
    setSendResult("");
    setSendError("");
    setSendOpen(true);
  };
  const reachableRelayCount = relayChecks.filter((item) => item.ok).length;
  const relayCheckSummary = t("relayPoolCheckSummary")
    .replace("{{ok}}", String(reachableRelayCount))
    .replace("{{total}}", String(relayChecks.length));

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold">{t("serverCommands")}</h1>
          <p className="mt-1 text-sm text-kumo-subtle">{t("serverCmdTips")}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ServerStatus
            label="ID"
            value={status[ID_TARGET]}
            onRefresh={() => void structured.refetch()}
          />
          <ServerStatus
            label="RELAY"
            value={status[RELAY_TARGET]}
            onRefresh={() => void structured.refetch()}
          />
          <Button onClick={openCreate}>{t("create")}</Button>
          <Button
            variant="secondary"
            disabled={!status[ID_TARGET]}
            onClick={() => openSend({ cmd: "", option: "", target: ID_TARGET })}
          >
            {t("sendToId")}
          </Button>
          <Button
            variant="secondary"
            disabled={!status[RELAY_TARGET]}
            onClick={() =>
              openSend({ cmd: "", option: "", target: RELAY_TARGET })
            }
          >
            {t("sendToRelay")}
          </Button>
        </div>
      </div>

      <section className="mb-4">
        <div className="mb-4 flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h2 className="text-base font-semibold">{t("structuredControls")}</h2>
            <p className="mt-1 text-sm text-kumo-subtle">
              {t("structuredControlsHint")}
            </p>
          </div>
          <Button
            variant="secondary"
            onClick={() => void structured.refetch()}
            loading={structured.isFetching}
          >
            {t("queryServerState")}
          </Button>
        </div>
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
          <div className="grid gap-4">
            <div className="rounded-lg border border-kumo-line bg-kumo-base p-4">
              <div className="mb-3 flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                <div>
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="text-sm font-semibold">{t("relayPool")}</h3>
                    <Badge variant={relayPool.data?.persisted ? "success" : "secondary"}>
                      {relayPool.data?.persisted
                        ? t("relayPoolPersisted")
                        : t("relayPoolDefault")}
                    </Badge>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={t("relayPoolInfo")}
                      onClick={() => setRulesOpen(true)}
                    >
                      <Info size={16} aria-hidden />
                    </Button>
                  </div>
                  <p className="mt-1 text-xs leading-5 text-kumo-subtle">
                    {t("relayPoolHint")}
                  </p>
                </div>
              </div>
              <label className="block">
                <span className="mb-1 block text-sm">{t("relayServersValue")}</span>
                <textarea
                  aria-label={t("relayServersValue")}
                  className="min-h-32 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 font-mono text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                  value={relayServers}
                  placeholder={"relay-a.example.com\n192.0.2.20:21117"}
                  onBlur={() => setRelayServers((value) => relayLines(value))}
                  onChange={(e) => {
                    setRelayServers(e.target.value);
                    setControlError("");
                  }}
                />
                <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
                  {t("relayPoolTextareaHint")}
                </span>
              </label>
              <div className="mt-3 flex flex-wrap gap-2">
                <Button
                  variant="secondary"
                  disabled={!relayLines(relayServers) || checkRelayServers.isPending}
                  loading={checkRelayServers.isPending}
                  onClick={() => {
                    const next = relayLines(relayServers);
                    if (!next) {
                      setControlError(t("relayPoolEmptyCheck"));
                      return;
                    }
                    setRelayServers(next);
                    checkRelayServers.mutate(next);
                  }}
                >
                  {t("checkRelayServers")}
                </Button>
                <Button
                  disabled={!status[ID_TARGET]}
                  loading={updateRelayServers.isPending}
                  onClick={() => {
                    const next = relayLines(relayServers);
                    setRelayServers(next);
                    updateRelayServers.mutate(next);
                  }}
                >
                  {t("applyRelayServers")}
                </Button>
                <Button
                  variant="secondary"
                  disabled={!status[ID_TARGET] || updateRelayServers.isPending}
                  onClick={() => {
                    setRelayServers("");
                    updateRelayServers.mutate("");
                  }}
                >
                  {t("restoreRelayDefault")}
                </Button>
              </div>
              {relayChecks.length > 0 && (
                <div className="mt-3 rounded-lg border border-kumo-line bg-kumo-elevated">
                  <div className="flex flex-wrap items-center justify-between gap-2 border-b border-kumo-line px-3 py-2">
                    <span className="text-sm font-medium">
                      {t("relayPoolCheckResults")}
                    </span>
                    <span className="text-xs text-kumo-subtle">
                      {relayCheckSummary}
                    </span>
                  </div>
                  <div className="divide-y divide-kumo-line">
                    {relayChecks.map((item) => (
                      <div
                        key={item.server}
                        className="grid gap-1 px-3 py-2 text-xs sm:grid-cols-[minmax(0,220px)_auto_minmax(0,1fr)] sm:items-center sm:gap-3"
                      >
                        <span className="break-all font-mono text-kumo-default">
                          {item.server}
                        </span>
                        <Badge variant={item.ok ? "success" : "error"}>
                          {item.ok ? t("ok") : t("error")}
                        </Badge>
                        <span className="break-all text-kumo-subtle">
                          {item.message}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>

            <div className="grid gap-4 lg:grid-cols-2">
              <div className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                <div className="text-xs font-semibold uppercase text-kumo-subtle">
                  {t("relayPoolCurrentRuntime")}
                </div>
                <div className="mt-2 min-h-10 break-words rounded-md border border-kumo-line bg-kumo-elevated px-3 py-2 font-mono text-xs">
                  {structured.data?.relay_servers?.trim() || "—"}
                </div>
              </div>
              <div className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                <div className="text-xs font-semibold uppercase text-kumo-subtle">
                  {t("relayPoolSavedValue")}
                </div>
                <div className="mt-2 min-h-10 whitespace-pre-wrap break-words rounded-md border border-kumo-line bg-kumo-elevated px-3 py-2 font-mono text-xs">
                  {relayPool.data?.persisted
                    ? relayPool.data.servers.join("\n")
                    : "—"}
                </div>
              </div>
            </div>

            <div className="rounded-lg border border-kumo-line bg-kumo-base">
              <button
                type="button"
                className="flex min-h-11 w-full cursor-pointer items-center justify-between gap-3 px-4 py-2 text-left"
                aria-expanded={advancedOpen}
                onClick={() => setAdvancedOpen((open) => !open)}
              >
                <span>
                  <span className="block text-sm font-medium">
                    {t("advancedRelayOptions")}
                  </span>
                  <span className="block text-xs leading-5 text-kumo-subtle">
                    {t("advancedRelayOptionsHint")}
                  </span>
                </span>
                <span className="text-sm text-kumo-subtle" aria-hidden="true">
                  {advancedOpen ? "−" : "+"}
                </span>
              </button>
              {advancedOpen && (
                <div className="grid gap-4 border-t border-kumo-line p-4">
                  <div className="grid gap-2">
                    <span className="text-sm font-medium">
                      {t("alwaysUseRelay")}
                    </span>
                    <p className="text-xs leading-5 text-kumo-subtle">
                      {t("alwaysUseRelayHint")}
                    </p>
                    <div className="flex flex-wrap items-center gap-3">
                      <label className="inline-flex min-h-9 items-center gap-2 rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm">
                        <input
                          type="checkbox"
                          checked={alwaysUseRelay}
                          onChange={(e) => {
                            setAlwaysUseRelay(e.target.checked);
                            setControlError("");
                          }}
                        />
                        {alwaysUseRelay ? t("enabled") : t("disabled")}
                      </label>
                      <Button
                        variant="secondary"
                        disabled={!status[ID_TARGET]}
                        loading={updateAlwaysUseRelay.isPending}
                        onClick={() => updateAlwaysUseRelay.mutate()}
                      >
                        {t("applyAlwaysUseRelay")}
                      </Button>
                    </div>
                  </div>
                  <div className="grid gap-2">
                    <span className="text-sm font-medium">
                      {t("ipBlockerSnapshot")}
                    </span>
                    <p className="text-xs leading-5 text-kumo-subtle">
                      {t("ipBlockerSnapshotHint")}
                    </p>
                    <Button
                      className="w-fit"
                      variant="secondary"
                      disabled={!status[ID_TARGET]}
                      loading={queryIpBlocker.isPending}
                      onClick={() => queryIpBlocker.mutate()}
                    >
                      {t("ipBlockerSnapshot")}
                    </Button>
                  </div>
                </div>
              )}
            </div>
          </div>
          <div>
            <span className="mb-1 block text-sm">{t("result")}</span>
            <pre className="max-h-56 min-h-24 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-kumo-line bg-kumo-base p-3 text-xs text-kumo-default">
              {controlError || controlMessage || "—"}
            </pre>
          </div>
        </div>
      </section>

      <Dialog.Root open={rulesOpen} onOpenChange={setRulesOpen}>
        <Dialog size="base" className={dialogPanelClass}>
          <DialogHeader
            title={t("relayPoolRules")}
            description={t("relayPoolHint")}
          />
          <DialogBody>
            <ul className="grid gap-3 text-sm leading-6 text-kumo-subtle">
              <li>{t("relayPoolRuleAddress")}</li>
              <li>{t("relayPoolRuleKey")}</li>
              <li>{t("relayPoolRuleClient")}</li>
              <li>{t("relayPoolRulePersist")}</li>
              <li>{t("relayPoolRuleClear")}</li>
            </ul>
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={() => setRulesOpen(false)}>
              {t("close")}
            </Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>

      <div className="overflow-x-auto rounded-lg border border-kumo-line">
        <Table>
          <Table.Header>
            <Table.Row>
              <Table.Head>{t("cmd")}</Table.Head>
              <Table.Head>{t("alias")}</Table.Head>
              <Table.Head>{t("option")}</Table.Head>
              <Table.Head>{t("target")}</Table.Head>
              <Table.Head>{t("explain")}</Table.Head>
              <Table.Head>{t("actions")}</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {(list.data?.list ?? []).map((row, i) => (
              <Table.Row key={`${row.id}-${row.cmd}-${i}`}>
                <Table.Cell>{row.cmd}</Table.Cell>
                <Table.Cell>{row.alias}</Table.Cell>
                <Table.Cell>{row.option}</Table.Cell>
                <Table.Cell>{targetLabel(row.target)}</Table.Cell>
                <Table.Cell>{row.explain}</Table.Cell>
                <Table.Cell>
                  <div className="flex flex-wrap gap-1">
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={!status[row.target] || row.cmd.trim() === ""}
                      onClick={() => openSend(row)}
                    >
                      {t("send")}
                    </Button>
                    {row.id > 0 && (
                      <>
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
                      </>
                    )}
                  </div>
                </Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table>
        {list.isLoading && (
          <TableState tone="loading">{t("loading")}</TableState>
        )}
        {list.error && (
          <TableState tone="error">
            {(list.error as Error).message || t("operationFailed")}
          </TableState>
        )}
        {!list.isLoading && !list.error && (list.data?.list ?? []).length === 0 && (
          <TableState tone="empty">{t("noData")}</TableState>
        )}
      </div>

      <Dialog.Root open={formOpen} onOpenChange={setFormOpen}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader
            title={`${form.id ? t("edit") : t("create")} · ${t("serverCommands")}`}
            description={t("commandDialogHint")}
          />
          <DialogBody>
            <div className="grid gap-4">
              <CommandFields form={form} onChange={setForm} />
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
              onClick={() => save.mutate()}
              loading={save.isPending}
              disabled={save.isPending || form.cmd.trim() === ""}
            >
              {t("save")}
            </Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>

      <Dialog.Root open={sendOpen} onOpenChange={setSendOpen}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader
            title={`${t("sendCmd")} · ${targetLabel(sendForm.target)}`}
            description={t("sendCommandHint")}
          />
          <DialogBody>
            <div className="grid gap-4">
              <label className="block">
                <span className="mb-1 block text-sm">{t("cmd")}</span>
                <Input
                  aria-label={t("cmd")}
                  value={sendForm.cmd}
                  onChange={(e) =>
                    setSendForm((s) => ({ ...s, cmd: e.target.value }))
                  }
                />
              </label>
              <label className="block">
                <span className="mb-1 block text-sm">{t("option")}</span>
                <Input
                  aria-label={t("option")}
                  value={sendForm.option}
                  onChange={(e) =>
                    setSendForm((s) => ({ ...s, option: e.target.value }))
                  }
                />
                {sendForm.explain && (
                  <span className="mt-1 block text-xs text-kumo-subtle">
                    {t("example")}: {sendForm.explain}
                  </span>
                )}
              </label>
              <label className="block">
                <span className="mb-1 block text-sm">{t("result")}</span>
                <textarea
                  className="min-h-48 w-full rounded-lg border border-kumo-line bg-kumo-base px-3 py-2 font-mono text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                  value={sendResult}
                  readOnly
                />
              </label>
            </div>
          </DialogBody>
          <DialogFooter error={sendError || undefined}>
            <Button variant="secondary" onClick={() => setSendOpen(false)}>
              {t("close")}
            </Button>
            <Button
              onClick={() => send.mutate()}
              loading={send.isPending}
              disabled={send.isPending || sendForm.cmd.trim() === ""}
            >
              {t("send")}
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
          remove.mutate(deleteTarget.id, {
            onSuccess: () => setDeleteTarget(null),
          });
        }}
      />
    </div>
  );
}

function ServerStatus({
  label,
  value,
  onRefresh,
}: {
  label: string;
  value: boolean | null;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2 rounded-md border border-kumo-line px-2 py-1 text-sm">
      <span>{label}</span>
      <Badge>{value === null ? t("checking") : value ? t("available") : t("notAvailable")}</Badge>
      <Button size="sm" variant="ghost" onClick={onRefresh}>
        {t("refresh")}
      </Button>
    </div>
  );
}

function CommandFields({
  form,
  onChange,
}: {
  form: CommandForm;
  onChange: React.Dispatch<React.SetStateAction<CommandForm>>;
}) {
  const { t } = useTranslation();
  return (
    <>
      <label className="block">
        <span className="mb-1 block text-sm">{t("cmd")}</span>
        <Input
          aria-label={t("cmd")}
          value={form.cmd}
          onChange={(e) => onChange((s) => ({ ...s, cmd: e.target.value }))}
        />
      </label>
      <label className="block">
        <span className="mb-1 block text-sm">{t("alias")}</span>
        <Input
          aria-label={t("alias")}
          value={form.alias}
          onChange={(e) => onChange((s) => ({ ...s, alias: e.target.value }))}
        />
      </label>
      <label className="block">
        <span className="mb-1 block text-sm">{t("option")}</span>
        <Input
          aria-label={t("option")}
          value={form.option}
          onChange={(e) => onChange((s) => ({ ...s, option: e.target.value }))}
        />
      </label>
      <label className="block">
        <span className="mb-1 block text-sm">{t("target")}</span>
        <select
          className="h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
          value={form.target}
          onChange={(e) => onChange((s) => ({ ...s, target: e.target.value }))}
        >
          <option value={ID_TARGET}>ID</option>
          <option value={RELAY_TARGET}>RELAY</option>
        </select>
      </label>
      <label className="block">
        <span className="mb-1 block text-sm">{t("explain")}</span>
        <Input
          aria-label={t("explain")}
          value={form.explain}
          onChange={(e) =>
            onChange((s) => ({ ...s, explain: e.target.value }))
          }
        />
      </label>
    </>
  );
}
