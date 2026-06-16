import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
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
          <p className="mt-1 text-sm text-color-muted">{t("serverCmdTips")}</p>
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

      <div className="overflow-x-auto rounded-lg border border-color-border">
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
                          variant="ghost"
                          onClick={() => {
                            if (confirm(t("confirmDelete")))
                              remove.mutate(row.id);
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
          <div className="p-4 text-sm text-color-muted">{t("loading")}</div>
        )}
        {!list.isLoading && (list.data?.list ?? []).length === 0 && (
          <div className="p-4 text-sm text-color-muted">{t("noData")}</div>
        )}
      </div>

      <Dialog.Root open={formOpen} onOpenChange={setFormOpen}>
        <Dialog>
          <Dialog.Title>
            {form.id ? t("edit") : t("create")} · {t("serverCommands")}
          </Dialog.Title>
          <div className="mt-4 space-y-3">
            <CommandFields form={form} onChange={setForm} />
            {save.error && (
              <p className="text-sm text-red-500">
                {(save.error as Error).message || t("operationFailed")}
              </p>
            )}
          </div>
          <div className="mt-6 flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setFormOpen(false)}>
              {t("cancel")}
            </Button>
            <Button onClick={() => save.mutate()} disabled={save.isPending}>
              {t("save")}
            </Button>
          </div>
        </Dialog>
      </Dialog.Root>

      <Dialog.Root open={sendOpen} onOpenChange={setSendOpen}>
        <Dialog>
          <Dialog.Title>
            {t("sendCmd")} · {targetLabel(sendForm.target)}
          </Dialog.Title>
          <div className="mt-4 space-y-3">
            <label className="block">
              <span className="mb-1 block text-sm">{t("cmd")}</span>
              <Input
                value={sendForm.cmd}
                onChange={(e) =>
                  setSendForm((s) => ({ ...s, cmd: e.target.value }))
                }
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-sm">{t("option")}</span>
              <Input
                value={sendForm.option}
                onChange={(e) =>
                  setSendForm((s) => ({ ...s, option: e.target.value }))
                }
              />
              {sendForm.explain && (
                <span className="mt-1 block text-xs text-color-muted">
                  {t("example")}: {sendForm.explain}
                </span>
              )}
            </label>
            <label className="block">
              <span className="mb-1 block text-sm">{t("result")}</span>
              <textarea
                className="min-h-48 w-full rounded-lg border border-color-border bg-kumo-base px-3 py-2 font-mono text-sm"
                value={sendResult}
                readOnly
              />
            </label>
            {sendError && <p className="text-sm text-red-500">{sendError}</p>}
          </div>
          <div className="mt-6 flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setSendOpen(false)}>
              {t("close")}
            </Button>
            <Button onClick={() => send.mutate()} disabled={send.isPending}>
              {t("send")}
            </Button>
          </div>
        </Dialog>
      </Dialog.Root>
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
    <div className="flex items-center gap-2 rounded-md border border-color-border px-2 py-1 text-sm">
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
          value={form.cmd}
          onChange={(e) => onChange((s) => ({ ...s, cmd: e.target.value }))}
        />
      </label>
      <label className="block">
        <span className="mb-1 block text-sm">{t("alias")}</span>
        <Input
          value={form.alias}
          onChange={(e) => onChange((s) => ({ ...s, alias: e.target.value }))}
        />
      </label>
      <label className="block">
        <span className="mb-1 block text-sm">{t("option")}</span>
        <Input
          value={form.option}
          onChange={(e) => onChange((s) => ({ ...s, option: e.target.value }))}
        />
      </label>
      <label className="block">
        <span className="mb-1 block text-sm">{t("target")}</span>
        <select
          className="h-9 w-full rounded-lg border border-color-border bg-kumo-elevated px-3 text-sm"
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
          value={form.explain}
          onChange={(e) =>
            onChange((s) => ({ ...s, explain: e.target.value }))
          }
        />
      </label>
    </>
  );
}
