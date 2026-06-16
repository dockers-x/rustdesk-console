import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
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
import { apiGet, apiPost, ApiError } from "../lib/api";

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

  const refreshStatus = async (target: string) => {
    setStatus((s) => ({ ...s, [target]: null }));
    try {
      await apiPost<string>("/api/admin/rustdesk/sendCmd", {
        cmd: "h",
        option: "",
        target,
      });
      setStatus((s) => ({ ...s, [target]: true }));
    } catch {
      setStatus((s) => ({ ...s, [target]: false }));
    }
  };

  useEffect(() => {
    void refreshStatus(ID_TARGET);
    void refreshStatus(RELAY_TARGET);
  }, []);

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
      option: "",
      explain: row.option ? `${row.cmd} ${row.option}` : row.cmd,
      target: row.target,
    });
    setSendResult("");
    setSendError("");
    setSendOpen(true);
  };

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
            onRefresh={() => void refreshStatus(ID_TARGET)}
          />
          <ServerStatus
            label="RELAY"
            value={status[RELAY_TARGET]}
            onRefresh={() => void refreshStatus(RELAY_TARGET)}
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
                      disabled={!status[row.target]}
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
            <Button onClick={() => save.mutate()} loading={save.isPending}>
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
            <Button onClick={() => send.mutate()} loading={send.isPending}>
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
