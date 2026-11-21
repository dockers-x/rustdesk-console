import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Checkbox } from "@cloudflare/kumo/components/checkbox";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input, Textarea } from "@cloudflare/kumo/components/input";
import { Select } from "@cloudflare/kumo/components/select";
import { Table } from "@cloudflare/kumo/components/table";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPut } from "../lib/api";
import { getAuthSessionId } from "../lib/auth";

interface Resource { id: number; name: string }
interface Collection extends Resource { note: string }
interface Scopes { groups: Resource[]; collections: Collection[] }
interface Page<T> { list: T[]; total: number }
interface User { id: number; username: string; nickname: string; status: number }
interface Entry { row_id: number; id: string; alias: string; note: string }

const endpoint = "/api/admin/delegated";
const pageSize = 20;
const surface = "min-w-0 rounded-lg border border-kumo-line bg-kumo-elevated";

function delegatedKey() {
  return ["delegated", getAuthSessionId()];
}

export function DelegatedAdminPage() {
  const { t } = useTranslation();
  const scopes = useQuery({
    queryKey: [...delegatedKey(), "scopes"],
    queryFn: () => apiGet<Scopes>(`${endpoint}/scopes`),
  });

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-semibold">{t("delegationTitle")}</h1>
      {scopes.isPending && <TableState tone="loading">{t("loading")}</TableState>}
      {scopes.isError && <InlineMessage tone="error">{scopes.error.message}</InlineMessage>}
      {scopes.isSuccess && <DelegatedResources scopes={scopes.data} />}
    </div>
  );
}

function DelegatedResources({ scopes: { groups, collections } }: { scopes: Scopes }) {
  const { t } = useTranslation();
  const [groupId, setGroupId] = useState<number | undefined>(() => groups[0]?.id);
  const [collectionId, setCollectionId] = useState<number | undefined>(() => collections[0]?.id);
  const group = groups.find(({ id }) => id === groupId) ?? groups[0];
  const collection = collections.find(({ id }) => id === collectionId) ?? collections[0];

  if (!group && !collection) return <TableState tone="empty">{t("delegationEmpty")}</TableState>;

  return (
    <>
      {group && (
        <section className="space-y-3" aria-labelledby="delegated-groups">
          <h2 id="delegated-groups" className="text-lg font-semibold">{t("delegationUserGroups")}</h2>
          <p className="text-sm text-kumo-subtle">{t("delegationUsersHint")}</p>
          <ResourceSelect label={t("delegationUserGroup")} resources={groups} value={group.id} onChange={setGroupId} />
          <GroupUsers key={group.id} groupId={group.id} />
        </section>
      )}
      {collection && (
        <section className="space-y-3" aria-labelledby="delegated-collections">
          <h2 id="delegated-collections" className="text-lg font-semibold">{t("delegationSharedCollections")}</h2>
          <p className="text-sm text-kumo-subtle">{t("delegationCollectionsHint")}</p>
          <ResourceSelect label={t("delegationSharedCollection")} resources={collections} value={collection.id} onChange={setCollectionId} />
          <CollectionEntries key={collection.id} collection={collection} />
        </section>
      )}
    </>
  );
}

function ResourceSelect({ label, resources, value, onChange }: {
  label: string;
  resources: Resource[];
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <div className="max-w-sm">
      <Select<number>
        label={label}
        value={value}
        items={resources.map(({ id, name }) => ({ value: id, label: name }))}
        onValueChange={(id) => { if (id !== null) onChange(id); }}
      >
        {resources.map(({ id, name }) => <Select.Option key={id} value={id}>{name}</Select.Option>)}
      </Select>
    </div>
  );
}

function Pager({ page, total, change }: { page: number; total: number; change: (page: number) => void }) {
  const { t } = useTranslation();
  const last = Math.max(1, Math.ceil(total / pageSize));
  if (page === 1 && last === 1) return null;
  return (
    <div className="flex flex-wrap items-center justify-end gap-2 border-t border-kumo-line px-4 py-3 text-sm">
      <span className="text-kumo-subtle">{page} / {last}</span>
      <Button size="sm" variant="secondary" disabled={page <= 1} onClick={() => change(page - 1)}>{t("consolePreviousPage")}</Button>
      <Button size="sm" variant="secondary" disabled={page >= last} onClick={() => change(page + 1)}>{t("consoleNextPage")}</Button>
    </div>
  );
}

function GroupUsers({ groupId }: { groupId: number }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const toast = useKumoToastManager();
  const key = delegatedKey();
  const path = `${endpoint}/groups/${groupId}/users`;
  const [page, setPage] = useState(1);
  const [selected, setSelected] = useState<number[]>([]);
  const [confirmDisable, setConfirmDisable] = useState(false);
  const users = useQuery({
    queryKey: [...key, path, page],
    queryFn: () => apiGet<Page<User>>(path, { page, page_size: pageSize }),
  });
  const selection = users.data?.list.filter((user) => selected.includes(user.id)) ?? [];
  const update = useMutation({
    mutationFn: (enabled: boolean) => apiPut(`${path}/status`, { enabled, user_ids: selection.map(({ id }) => id) }),
    onSuccess: async () => {
      setSelected([]);
      setConfirmDisable(false);
      await client.invalidateQueries({ queryKey: [...key, path] });
      toast.add({ title: t("operationSuccess"), variant: "success" });
    },
  });

  return (
    <div className={surface}>
      <div className="flex flex-wrap gap-2 border-b border-kumo-line px-4 py-3">
        <Button size="sm" variant="secondary"
          disabled={users.isError || update.isPending || !selection.some(({ status }) => status !== 1)}
          onClick={() => update.mutate(true)}>{t("delegationEnableUsers")}</Button>
        <Button size="sm" variant="secondary"
          disabled={users.isError || update.isPending || !selection.some(({ status }) => status === 1)}
          onClick={() => { update.reset(); setConfirmDisable(true); }}>{t("delegationDisableUsers")}</Button>
      </div>
      {update.isError && !confirmDisable && <InlineMessage tone="error">{update.error.message}</InlineMessage>}
      {users.isSuccess && users.data.list.length > 0 && (
        <div className="overflow-x-auto">
          <Table>
            <Table.Header><Table.Row>
              <Table.Head>{t("delegationSelectUser")}</Table.Head>
              <Table.Head>{t("username")}</Table.Head>
              <Table.Head>{t("nickname")}</Table.Head>
              <Table.Head>{t("status")}</Table.Head>
            </Table.Row></Table.Header>
            <Table.Body>
              {users.data.list.map((user) => (
                <Table.Row key={user.id}>
                  <Table.Cell><Checkbox
                    aria-label={t("delegationSelectAccount", { username: user.username })}
                    checked={selected.includes(user.id)}
                    disabled={update.isPending}
                    onCheckedChange={(checked) => setSelected((ids) => checked ? [...ids, user.id] : ids.filter((id) => id !== user.id))}
                  /></Table.Cell>
                  <Table.Cell>{user.username}</Table.Cell>
                  <Table.Cell>{user.nickname || "—"}</Table.Cell>
                  <Table.Cell><Badge>{t(user.status === 1 ? "enabled" : "disabled")}</Badge></Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
        </div>
      )}
      {users.isPending && <TableState tone="loading">{t("loading")}</TableState>}
      {users.isError && <TableState tone="error">{users.error.message}</TableState>}
      {users.isSuccess && users.data.list.length === 0 && <TableState tone="empty">{t("noData")}</TableState>}
      <Pager page={page} total={users.data?.total ?? 0} change={(next) => { setSelected([]); setPage(next); }} />
      <ConfirmDialog open={confirmDisable}
        title={t("delegationDisableUsers")} description={t("delegationDisableConfirm")}
        confirmLabel={t("delegationDisableUsers")} cancelLabel={t("cancel")}
        error={update.error?.message} loading={update.isPending}
        onOpenChange={(open) => { if (!update.isPending) setConfirmDisable(open); }}
        onConfirm={() => update.mutate(false)}
      />
    </div>
  );
}

function CollectionEntries({ collection }: { collection: Collection }) {
  const { t } = useTranslation();
  const key = delegatedKey();
  const [page, setPage] = useState(1);
  const [edit, setEdit] = useState<Entry | "collection">();
  const path = `${endpoint}/collections/${collection.id}`;
  const entries = useQuery({
    queryKey: [...key, path, page],
    queryFn: () => apiGet<Page<Entry>>(`${path}/entries`, { page, page_size: pageSize }),
  });

  return (
    <div className={surface}>
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-kumo-line px-4 py-3">
        <p className="max-w-2xl break-words whitespace-pre-wrap text-sm text-kumo-subtle">{collection.note || "—"}</p>
        <Button size="sm" variant="secondary" onClick={() => setEdit("collection")}>{t("delegationEditCollection")}</Button>
      </div>
      {entries.isSuccess && entries.data.list.length > 0 && (
        <div className="overflow-x-auto">
          <Table>
            <Table.Header><Table.Row>
              <Table.Head>{t("deviceId")}</Table.Head>
              <Table.Head>{t("alias")}</Table.Head>
              <Table.Head>{t("note")}</Table.Head>
              <Table.Head>{t("actions")}</Table.Head>
            </Table.Row></Table.Header>
            <Table.Body>
              {entries.data.list.map((entry) => (
                <Table.Row key={entry.row_id}>
                  <Table.Cell><span className="font-mono">{entry.id}</span></Table.Cell>
                  <Table.Cell>{entry.alias || "—"}</Table.Cell>
                  <Table.Cell><span className="block max-w-sm break-words whitespace-pre-wrap">{entry.note || "—"}</span></Table.Cell>
                  <Table.Cell><Button size="sm" variant="secondary"
                    aria-label={t("delegationEditEntryFor", { id: entry.id })}
                    onClick={() => setEdit(entry)}>{t("edit")}</Button></Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
        </div>
      )}
      {entries.isPending && <TableState tone="loading">{t("loading")}</TableState>}
      {entries.isError && <TableState tone="error">{entries.error.message}</TableState>}
      {entries.isSuccess && entries.data.list.length === 0 && <TableState tone="empty">{t("noData")}</TableState>}
      <Pager page={page} total={entries.data?.total ?? 0} change={setPage} />
      <EditCollectionDetails
        path={path}
        collection={collection}
        edit={edit}
        onClose={() => setEdit(undefined)}
      />
    </div>
  );
}

function EditCollectionDetails({ path, collection, edit, onClose }: {
  path: string;
  collection: Collection;
  edit?: Entry | "collection";
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const toast = useKumoToastManager();
  const key = delegatedKey();
  const isCollection = edit === "collection";
  const save = useMutation({
    mutationFn: ({ value, note }: { value: string; note: string }) => isCollection
      ? apiPut(path, { name: value, note })
      : apiPut(`${path}/entries/${edit && edit.row_id}`, { alias: value, note }),
    onSuccess: async () => {
      onClose();
      await client.invalidateQueries({ queryKey: [...key, isCollection ? "scopes" : path] });
      toast.add({ title: t("operationSuccess"), variant: "success" });
    },
  });

  return (
    <Dialog.Root open={Boolean(edit)} onOpenChange={(open) => {
      if (!open && !save.isPending) { save.reset(); onClose(); }
    }}>
      <Dialog className={dialogPanelClass}>
        <DialogHeader title={t(isCollection ? "delegationEditCollection" : "delegationEditEntry")} />
        {edit && (
          <form className="flex min-h-0 flex-col" onSubmit={(event) => {
            event.preventDefault();
            const data = new FormData(event.currentTarget);
            save.mutate({ value: String(data.get("value") ?? ""), note: String(data.get("note") ?? "") });
          }}>
            <DialogBody>
              <div className="grid gap-4">
                <Input label={t(isCollection ? "name" : "alias")} name="value" maxLength={255}
                  defaultValue={isCollection ? collection.name : edit.alias} required={isCollection} />
                <Textarea label={t("note")} name="note" rows={4} maxLength={255}
                  defaultValue={isCollection ? collection.note : edit.note} />
              </div>
            </DialogBody>
            <DialogFooter error={save.error?.message}>
              <Button type="button" variant="secondary" disabled={save.isPending} onClick={() => { save.reset(); onClose(); }}>{t("cancel")}</Button>
              <Button type="submit" variant="primary" loading={save.isPending}>{t("save")}</Button>
            </DialogFooter>
          </form>
        )}
      </Dialog>
    </Dialog.Root>
  );
}
