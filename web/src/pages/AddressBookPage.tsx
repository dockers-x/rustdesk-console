import { useState, type FormEvent, type ReactNode } from "react";
import { isAxiosError } from "axios";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Link, useSearchParams } from "react-router-dom";
import { ArrowClockwise, ArrowLeft, DownloadSimple, Plus, Trash } from "@phosphor-icons/react";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Checkbox } from "@cloudflare/kumo/components/checkbox";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { LayerDialog } from "@cloudflare/kumo/components/layer-dialog";
import { Radio } from "@cloudflare/kumo/components/radio";
import { Table } from "@cloudflare/kumo/components/table";
import { Tabs } from "@cloudflare/kumo/components/tabs";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass, resourceFormDialogPanelClass } from "../components/DialogLayout";
import { TableState } from "../components/TableState";
import { TagPicker } from "../components/TagPicker";
import { WebClientActions } from "../components/WebClientActions";
import { apiGet, apiPatch, apiPost, apiPut, http } from "../lib/api";
import { formatDateTime } from "../lib/dateFormat";

const pageSize = 20;
const permissionLabels = ["", "ruleRead", "ruleReadWrite", "ruleFull"];
const bookKey = ["address-book"];
const surface = "min-w-0 rounded-lg border border-kumo-line bg-kumo-elevated";

interface Page<T> {
  data: T[];
  total: number;
}

interface Book {
  guid: string;
  name: string;
  owner: string;
  note?: string;
  rule: number;
}

interface BookPeer {
  row_id: number;
  id: string;
  hostname: string;
  username: string;
  alias: string;
  note: string;
  tags: string[];
  deleted_at?: string;
}

interface BookTag {
  id: number;
  name: string;
  color: number;
}

interface Grant {
  guid: string;
  type: "user" | "group" | "everyone";
  user: string;
  group: string;
  rule: number;
}

function message(error: unknown, fallback: string) {
  if (isAxiosError(error) && typeof error.response?.data?.error === "string") {
    return error.response.data.error;
  }
  return error instanceof Error ? error.message : fallback;
}

function remove(path: string, data: unknown) {
  return http.delete(path, { data });
}

function Pager({ page, total, setPage }: {
  page: number;
  total: number;
  setPage: (page: number) => void;
}) {
  const { t } = useTranslation();
  const last = Math.max(1, Math.ceil(total / pageSize));
  return (
    <div className="flex flex-wrap items-center justify-end gap-2 px-4 py-3 text-sm">
      <span className="mr-1 text-kumo-subtle">{page} / {last}</span>
      <Button size="sm" variant="secondary" disabled={page <= 1} onClick={() => setPage(page - 1)}>
        {t("bookPrevious")}
      </Button>
      <Button size="sm" variant="secondary" disabled={page >= last} onClick={() => setPage(page + 1)}>
        {t("bookNext")}
      </Button>
    </div>
  );
}

function BookDialog({ title, onClose, onSubmit, pending, error, children, action }: {
  title: string;
  onClose: () => void;
  onSubmit: (event: FormEvent) => void;
  pending?: boolean;
  error?: string;
  children: ReactNode;
  action?: string;
}) {
  const { t } = useTranslation();
  return (
    <Dialog.Root open onOpenChange={(open) => { if (!open && !pending) onClose(); }}>
      <Dialog className={dialogPanelClass}>
        <form className="flex min-h-0 flex-col" onSubmit={onSubmit}>
          <DialogHeader title={title} />
          <DialogBody><div className="grid gap-4">{children}</div></DialogBody>
          <DialogFooter error={error}>
            <Button variant="secondary" type="button" disabled={pending} onClick={onClose}>{t("cancel")}</Button>
            <Button type="submit" loading={pending}>{action ?? t("save")}</Button>
          </DialogFooter>
        </form>
      </Dialog>
    </Dialog.Root>
  );
}

export function AddressBookPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [params, setParams] = useSearchParams();
  const shared = params.get("tab") === "shared";
  const selected = params.get("book") || "";
  const [page, setPage] = useState(1);
  const [nameFilter, setNameFilter] = useState("");
  const [filter, setFilter] = useState("");
  const [editing, setEditing] = useState<Book | "new" | null>(null);
  const [bookName, setBookName] = useState("");
  const [bookNote, setBookNote] = useState("");
  const [manage, setManage] = useState<Book | null>(null);
  const [deleteBook, setDeleteBook] = useState<Book | null>(null);

  const personal = useQuery({
    queryKey: [...bookKey, "personal"],
    queryFn: () => apiGet<Book>("/api/ab/personal"),
    enabled: !shared,
  });
  const profiles = useQuery({
    queryKey: [...bookKey, "profiles", page, filter],
    queryFn: () => apiGet<Page<Book>>("/api/ab/shared/profiles", { page, pageSize, name: filter }),
    enabled: shared && !selected,
  });
  const sharedBook = useQuery({
    queryKey: [...bookKey, "book", selected],
    queryFn: () => apiGet<Book>("/api/ab/shared/profile", { ab: selected }),
    enabled: shared && Boolean(selected),
  });

  const saveProfile = useMutation({
    mutationFn: async () => {
      const name = bookName.trim();
      const note = bookNote.trim();
      if (editing === "new") return apiPost("/api/ab/shared/add", { name, note });
      return apiPut("/api/ab/shared/update/profile", { guid: editing?.guid, name, note });
    },
    onSuccess: () => {
      setEditing(null);
      void qc.invalidateQueries({ queryKey: bookKey });
    },
  });
  const deleteProfile = useMutation({
    mutationFn: (book: Book) => remove("/api/ab/shared", [book.guid]),
    onSuccess: () => {
      setDeleteBook(null);
      void qc.invalidateQueries({ queryKey: bookKey });
    },
  });

  const current = shared ? sharedBook : personal;
  return (
    <div className="mx-auto grid max-w-[96rem] gap-5">
      <div className="grid gap-3">
        <h1 className="text-2xl font-semibold">{t("addressBook")}</h1>
        <Tabs
          value={shared ? "shared" : "personal"}
          onValueChange={(value) => setParams(value === "shared" ? { tab: "shared" } : {})}
          tabs={[
            { value: "personal", label: t("myAddressBook") },
            { value: "shared", label: t("bookShared") },
          ]}
        />
      </div>

      {shared && !selected ? (
        <section className={surface}>
          <div className="flex flex-wrap items-end justify-between gap-3 p-4">
            <form className="flex items-end gap-2" onSubmit={(event) => {
              event.preventDefault();
              setPage(1);
              setFilter(nameFilter);
            }}>
              <Input label={t("name")} value={nameFilter} onChange={(event) => setNameFilter(event.target.value)} />
              <Button type="submit" variant="secondary">{t("search")}</Button>
            </form>
            <Button onClick={() => { saveProfile.reset(); setBookName(""); setBookNote(""); setEditing("new"); }}>
              <Plus size={16} />{t("bookNewShared")}
            </Button>
          </div>
          <div className="overflow-x-auto">
          <Table>
            <Table.Header>
              <Table.Row>
                {[ "name", "bookOwner", "rule", "note", "actions" ].map((key) => <Table.Head key={key}>{t(key)}</Table.Head>)}
              </Table.Row>
            </Table.Header>
            <Table.Body>
              {profiles.data?.data.map((book) => (
                <Table.Row key={book.guid}>
                  <Table.Cell>
                    <Link className="font-medium text-kumo-brand hover:underline"
                      to={`?tab=shared&book=${encodeURIComponent(book.guid)}`}>{book.name}</Link>
                  </Table.Cell>
                  <Table.Cell>{book.owner}</Table.Cell>
                  <Table.Cell><Badge>{t(permissionLabels[book.rule] || "noData")}</Badge></Table.Cell>
                  <Table.Cell><span className="block max-w-64 break-words">{book.note || "—"}</span></Table.Cell>
                  <Table.Cell>
                    {book.rule === 3 && (
                      <div className="flex flex-wrap gap-1">
                        <Button size="sm" variant="ghost" onClick={() => setManage(book)}>{t("shareRules")}</Button>
                        <Button size="sm" variant="ghost" onClick={() => {
                          saveProfile.reset(); setBookName(book.name); setBookNote(book.note ?? ""); setEditing(book);
                        }}>{t("edit")}</Button>
                        <Button size="sm" variant="ghost" onClick={() => { deleteProfile.reset(); setDeleteBook(book); }}>
                          {t("delete")}
                        </Button>
                      </div>
                    )}
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
          </div>
          {profiles.isPending && <TableState tone="loading">{t("loading")}</TableState>}
          {profiles.error && <TableState tone="error">{message(profiles.error, t("operationFailed"))}</TableState>}
          {profiles.data?.total === 0 && <TableState tone="empty">{t("noData")}</TableState>}
          {profiles.data && <Pager page={page} total={profiles.data.total} setPage={setPage} />}
        </section>
      ) : (
        <>
          {shared && (
            <div className="flex flex-wrap items-center justify-between gap-2">
              <Button variant="ghost" onClick={() => setParams({ tab: "shared" })}>
                <ArrowLeft size={16} />{t("bookShared")}
              </Button>
              {sharedBook.data?.rule === 3 && <Button variant="secondary" onClick={() => { if (sharedBook.data) setManage(sharedBook.data); }}>{t("shareRules")}</Button>}
            </div>
          )}
          {current.isPending && <TableState tone="loading">{t("loading")}</TableState>}
          {current.error && <TableState tone="error">{message(current.error, t("operationFailed"))}</TableState>}
          {current.data && <BookDevices key={current.data.guid} book={current.data} />}
        </>
      )}

      {editing && (
        <BookDialog title={t(editing === "new" ? "bookNewShared" : "edit")}
          onClose={() => setEditing(null)} pending={saveProfile.isPending} error={saveProfile.error ? message(saveProfile.error, t("operationFailed")) : undefined}
          onSubmit={(event) => { event.preventDefault(); if (bookName.trim()) saveProfile.mutate(); }}>
          <Input label={t("name")} value={bookName} required maxLength={120}
            onChange={(event) => setBookName(event.target.value)} />
          <Input label={t("note")} value={bookNote} maxLength={255}
            onChange={(event) => setBookNote(event.target.value)} />
        </BookDialog>
      )}
      {manage && <GrantDialog key={manage.guid} book={manage} onClose={() => setManage(null)} />}
      {deleteBook && (
        <ConfirmDialog open title={t("confirmDeleteTitle")} description={t("bookDeleteDescription", { name: deleteBook.name })}
          confirmLabel={t("delete")} cancelLabel={t("cancel")} loading={deleteProfile.isPending}
          error={deleteProfile.error ? message(deleteProfile.error, t("operationFailed")) : undefined}
          onOpenChange={(open) => { if (!open) setDeleteBook(null); }} onConfirm={() => deleteProfile.mutate(deleteBook)} />
      )}
    </div>
  );
}

const peerFields = [
  ["id", "deviceId"], ["hostname", "bookDeviceName"], ["username", "username"],
  ["alias", "alias"], ["note", "note"],
] as const;

function BookDevices({ book }: { book: Book }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [filters, setFilters] = useState<Record<string, string>>({});
  const [applied, setApplied] = useState<Record<string, string>>({});
  const [tagFilter, setTagFilter] = useState<string>();
  const [page, setPage] = useState(1);
  const [editing, setEditing] = useState<BookPeer | "new" | null>(null);
  const [tagEditor, setTagEditor] = useState<BookTag | "new" | null>(null);
  const [deleteTag, setDeleteTag] = useState<BookTag>();
  const [trashOpen, setTrashOpen] = useState(false);
  const [deletePeer, setDeletePeer] = useState<BookPeer>();
  const writable = book.rule >= 2;
  const fullControl = book.rule === 3;
  const guid = encodeURIComponent(book.guid);
  const peers = useQuery({
    queryKey: [...bookKey, book.guid, "peers", applied, tagFilter, page],
    queryFn: () => apiGet<Page<BookPeer>>("/api/ab/peers", { ...applied, ab: book.guid, page, pageSize, tag: tagFilter }),
  });
  const tags = useQuery({
    queryKey: [...bookKey, book.guid, "tags"],
    queryFn: async () => {
      const data = await apiGet<BookTag[]>(`/api/ab/tags/${guid}`);
      if (!Array.isArray(data)) throw new Error(t("operationFailed"));
      return data;
    },
  });
  const deleteDevice = useMutation({
    mutationFn: (peer: BookPeer) => remove(`/api/ab/peer/${guid}`, [peer.id]),
    onSuccess: () => {
      setDeletePeer(undefined);
      void qc.invalidateQueries({ queryKey: [...bookKey, book.guid] });
    },
  });

  const selectedTag = tags.data?.find((tag) => tag.id > 0 && tag.name === tagFilter);
  const selectTagFilter = (tag?: string) => { setTagFilter(tag); setPage(1); };
  const deleteBookTag = useMutation({
    mutationFn: (tag: BookTag) => remove(`/api/ab/tag/${guid}`, [tag.name]),
    onSuccess: () => {
      setDeleteTag(undefined);
      setTagFilter(undefined);
      setPage(1);
      void qc.invalidateQueries({ queryKey: [...bookKey, book.guid] });
    },
  });

  return (
    <>
      {book.owner && (
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-lg font-semibold">{book.name}</h2>
          <Badge>{t(permissionLabels[book.rule] || "noData")}</Badge>
          <span className="text-sm text-kumo-subtle">{book.owner}</span>
        </div>
      )}
      <section className={`${surface} grid gap-3 p-4`} aria-label={t("tags")}>
        <div className="max-w-2xl">
          <TagPicker label={t("tags")} value={tagFilter ? [tagFilter] : []}
            options={(tags.data ?? []).map((tag) => tag.name)} maxValues={1}
            disabled={tags.isPending || tags.isError}
            onChange={([tag]) => {
              if (tag !== tagFilter && (tag !== undefined || tagFilter !== "")) selectTagFilter(tag);
            }} />
        </div>
        {tags.error && <TableState tone="error">{t("operationFailed")}</TableState>}
        <div className="flex flex-wrap gap-2">
          {[undefined, ""].map((tag) => (
          <Button key={tag === undefined ? "all-tags" : "no-tags"} size="sm"
            variant={tagFilter === tag ? "primary" : "secondary"} aria-pressed={tagFilter === tag}
            onClick={() => selectTagFilter(tag)}>
            {t(tag === undefined ? "bookAllTags" : "bookNoTags")}
          </Button>
          ))}
        {writable && <Button size="sm" variant="ghost" onClick={() => setTagEditor("new")}>
          <Plus size={16} />{t("bookAddTag")}
        </Button>}
        {writable && selectedTag && <Button size="sm" variant="ghost" onClick={() => setTagEditor(selectedTag)}>{t("edit")}</Button>}
        {fullControl && selectedTag && <Button size="sm" variant="ghost" onClick={() => {
          deleteBookTag.reset(); setDeleteTag(selectedTag);
        }}>{t("delete")}</Button>}
        </div>
      </section>
      <form className={`${surface} grid items-end gap-3 p-4 sm:grid-cols-2 xl:grid-cols-3`}
        onSubmit={(event) => { event.preventDefault(); setPage(1); setApplied(filters); }}>
        {peerFields.map(([field, key]) => (
          <Input key={field} label={t(key)} value={filters[field] ?? ""}
            onChange={(event) => setFilters({ ...filters, [field]: event.target.value })} />
        ))}
        <div className="flex justify-end gap-2">
          <Button type="button" variant="secondary" onClick={() => {
            setFilters({}); setApplied({}); setTagFilter(undefined); setPage(1);
          }}>{t("bookResetFilters")}</Button>
          <Button type="submit">{t("search")}</Button>
        </div>
      </form>
      <section className={surface}>
        <div className="flex flex-wrap items-center justify-between gap-3 p-4">
          <h2 className="text-sm font-semibold">{t("devices")}</h2>
          <div className="flex flex-wrap gap-2">
            {writable && <>
              <Button size="sm" onClick={() => setEditing("new")}><Plus size={16} />{t("create")}</Button>
              <ImportDialog book={book} />
            </>}
            {fullControl && <Button size="sm" variant="secondary" onClick={() => setTrashOpen(true)}>
              <Trash size={16} />{t("bookTrash")}
            </Button>}
            <Button size="sm" variant="ghost" aria-label={t("refresh")}
              onClick={() => void qc.invalidateQueries({ queryKey: [...bookKey, book.guid] })}>
              <ArrowClockwise size={16} />
            </Button>
          </div>
        </div>
        <DeviceTable rows={peers.data?.data ?? []} actions={(peer) => (
          <div className="flex flex-wrap items-center gap-1">
            <WebClientActions peerId={peer.id} />
            {writable && <Button size="xs" variant="ghost" onClick={() => setEditing(peer)}>{t("edit")}</Button>}
            {fullControl && <Button size="xs" variant="ghost" onClick={() => {
              deleteDevice.reset(); setDeletePeer(peer);
            }}>{t("delete")}</Button>}
          </div>
        )} />
        {peers.isPending && <TableState tone="loading">{t("loading")}</TableState>}
        {peers.error && <TableState tone="error">{message(peers.error, t("operationFailed"))}</TableState>}
        {peers.data?.total === 0 && <TableState tone="empty">{t("noData")}</TableState>}
        {peers.data && <Pager page={page} total={peers.data.total} setPage={setPage} />}
      </section>
      {editing && <PeerDialog book={book} peer={editing} tags={tags.data ?? []} onClose={() => setEditing(null)} />}
      {tagEditor && <TagDialog book={book} tag={tagEditor === "new" ? undefined : tagEditor}
        onSaved={(name) => { if (tagEditor !== "new") { setTagFilter(name); setPage(1); } }}
        onClose={() => setTagEditor(null)} />}
      {deleteTag && <ConfirmDialog open title={t("confirmDeleteTitle")}
        description={t("bookDeleteTagDescription", { name: deleteTag.name })}
        confirmLabel={t("delete")} cancelLabel={t("cancel")} loading={deleteBookTag.isPending}
        error={deleteBookTag.error ? message(deleteBookTag.error, t("operationFailed")) : undefined}
        onOpenChange={(open) => { if (!open) setDeleteTag(undefined); }}
        onConfirm={() => deleteBookTag.mutate(deleteTag)} />}
      {trashOpen && <TrashDialog book={book} onClose={() => setTrashOpen(false)} />}
      {deletePeer && (
        <ConfirmDialog open title={t("bookMoveToTrash")} description={t("bookMoveDescription", { id: deletePeer.id })}
          loading={deleteDevice.isPending} confirmLabel={t("delete")} cancelLabel={t("cancel")}
          error={deleteDevice.error ? message(deleteDevice.error, t("operationFailed")) : undefined}
          onOpenChange={(open) => { if (!open) setDeletePeer(undefined); }}
          onConfirm={() => deleteDevice.mutate(deletePeer)} />
      )}
    </>
  );
}

function DeviceTable({ rows, actions, trash = false }: {
  rows: BookPeer[];
  actions: (peer: BookPeer) => ReactNode;
  trash?: boolean;
}) {
  const { t } = useTranslation();
  const headers = ["deviceId", "devices", "alias", "tags", "note", ...(trash ? ["bookDeletedAt"] : []), "actions"];
  return (
    <div className="min-w-0 overflow-x-auto">
    <Table>
      <Table.Header>
        <Table.Row>{headers.map((key) => <Table.Head key={key}>{t(key)}</Table.Head>)}</Table.Row>
      </Table.Header>
      <Table.Body>
        {rows.map((peer) => (
          <Table.Row key={peer.row_id}>
            <Table.Cell><span className="font-mono">{peer.id}</span></Table.Cell>
            <Table.Cell>
              <span className="block">{peer.hostname || "—"}</span>
              {peer.username && <span className="text-xs text-kumo-subtle">{peer.username}</span>}
            </Table.Cell>
            <Table.Cell>{peer.alias || "—"}</Table.Cell>
            <Table.Cell><div className="flex flex-wrap gap-1">
              {Array.isArray(peer.tags) && peer.tags.map((tag) => <Badge key={tag}>{tag}</Badge>)}
            </div></Table.Cell>
            <Table.Cell><span className="block max-w-72 break-words whitespace-pre-wrap">{peer.note || "—"}</span></Table.Cell>
            {trash && <Table.Cell>{formatDateTime(peer.deleted_at)}</Table.Cell>}
            <Table.Cell>{actions(peer)}</Table.Cell>
          </Table.Row>
        ))}
      </Table.Body>
    </Table>
    </div>
  );
}

function PeerDialog({ book, peer, tags, onClose }: {
  book: Book;
  peer: BookPeer | "new";
  tags: BookTag[];
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const original = peer === "new" ? undefined : peer;
  const [form, setForm] = useState({
    id: original?.id ?? "",
    alias: original?.alias ?? "",
    note: original?.note ?? "",
    tags: Array.isArray(original?.tags) ? original.tags : [],
  });
  const save = useMutation({
    mutationFn: () => {
      const guid = encodeURIComponent(book.guid);
      const data = { ...form, id: form.id.trim() };
      return original ? apiPut(`/api/ab/peer/update/${guid}`, data) : apiPost(`/api/ab/peer/add/${guid}`, data);
    },
    onSuccess: () => { void qc.invalidateQueries({ queryKey: [...bookKey, book.guid] }); onClose(); },
  });
  return (
    <BookDialog title={t(original ? "edit" : "create")} pending={save.isPending}
      error={save.error ? message(save.error, t("operationFailed")) : undefined}
      onClose={onClose} onSubmit={(event) => { event.preventDefault(); if (form.id.trim()) save.mutate(); }}>
      <Input label={t("deviceId")} value={form.id} readOnly={Boolean(original)} required maxLength={128}
        onChange={(event) => setForm({ ...form, id: event.target.value })} />
      <Input label={t("alias")} value={form.alias} maxLength={255}
        onChange={(event) => setForm({ ...form, alias: event.target.value })} />
      <Input label={t("note")} value={form.note} maxLength={1000}
        onChange={(event) => setForm({ ...form, note: event.target.value })} />
      <TagPicker label={t("tags")} value={form.tags}
        options={tags.map((tag) => tag.name)}
        onChange={(value) => setForm({ ...form, tags: value })} />
    </BookDialog>
  );
}

function TagDialog({ book, tag, onSaved, onClose }: {
  book: Book;
  tag?: BookTag;
  onSaved: (name: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState(tag?.name ?? "");
  const [color, setColor] = useState(tag ? `#${(tag.color & 0xffffff).toString(16).padStart(6, "0")}` : "#2b70cc");
  const save = useMutation({
    mutationFn: () => {
      const guid = encodeURIComponent(book.guid);
      const value = Number.parseInt(`ff${color.slice(1)}`, 16);
      return tag
        ? apiPut(`/api/ab/tag/rename/${guid}`, { old: tag.name, new: name.trim(), color: value })
        : apiPost(`/api/ab/tag/add/${guid}`, { name: name.trim(), color: value });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: [...bookKey, book.guid] });
      onSaved(name.trim());
      onClose();
    },
  });
  return (
    <BookDialog title={t(tag ? "edit" : "bookAddTag")} pending={save.isPending}
      error={save.error ? message(save.error, t("operationFailed")) : undefined}
      onClose={onClose} onSubmit={(event) => { event.preventDefault(); if (name.trim()) save.mutate(); }}>
      <Input label={t("name")} value={name} required maxLength={80} onChange={(event) => setName(event.target.value)} />
      <Input type="color" label={t("color")} value={color} onChange={(event) => setColor(event.target.value)} />
    </BookDialog>
  );
}

function ImportDialog({ book }: { book: Book }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toasts = useKumoToastManager();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<number[]>([]);
  const peers = useQuery({
    queryKey: [...bookKey, "import-devices", search],
    enabled: open,
    queryFn: () => apiGet<{ list: BookPeer[] }>("/api/admin/my/peer/list", { id: search, page_size: 100 }),
  });
  const save = useMutation({
    mutationFn: () => apiPost(`/api/ab/peer/import/${encodeURIComponent(book.guid)}`, { peer_ids: selected }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: [...bookKey, book.guid] });
      setOpen(false);
      toasts.add({ title: t("bookImportDone"), variant: "success" });
    },
  });
  return (
    <LayerDialog open={open} dismissDisabled={save.isPending} onOpenChange={(next) => {
      if (next) {
        setSearch("");
        setSelected([]);
        save.reset();
      }
      setOpen(next);
    }}>
      <LayerDialog.Trigger render={<Button size="sm" variant="secondary" icon={DownloadSimple} />}>
        {t("bookImport")}
      </LayerDialog.Trigger>
      <LayerDialog.Content>
        <LayerDialog.Title>{t("bookImport")}</LayerDialog.Title>
        <LayerDialog.Body>
          <div className="grid gap-4">
            <Input className="w-full" label={t("deviceId")} value={search} onChange={(event) => setSearch(event.target.value)} />
            <fieldset className="min-w-0">
              <legend className="mb-2 text-sm font-medium">{t("devices")}</legend>
              <p className="mb-2 text-sm text-kumo-subtle" role="status">{t("bookImportSelected", { count: selected.length })}</p>
              {peers.isPending && <TableState tone="loading">{t("loading")}</TableState>}
              {peers.data?.list.length === 0 && <TableState tone="empty">{t("noData")}</TableState>}
              {peers.data?.list.length ? (
                <div className="grid max-h-64 gap-1 overflow-y-auto rounded-md border border-kumo-line p-2">
                  {peers.data.list.map((peer) => (
                    <Checkbox key={peer.row_id} checked={selected.includes(peer.row_id)}
                      label={<span className="break-all text-sm">{peer.hostname ? `${peer.hostname} · ${peer.id}` : peer.id}</span>}
                      onCheckedChange={(checked) => setSelected((ids) =>
                        checked ? [...ids, peer.row_id] : ids.filter((id) => id !== peer.row_id))} />
                  ))}
                </div>
              ) : null}
            </fieldset>
            {peers.error && <TableState tone="error">{message(peers.error, t("operationFailed"))}</TableState>}
            {save.error && <p role="alert" className="text-sm text-kumo-danger">{message(save.error, t("operationFailed"))}</p>}
          </div>
        </LayerDialog.Body>
        <LayerDialog.Actions dismissLabel={t("cancel")}>
          <LayerDialog.Action disabled={!selected.length} loading={save.isPending} onClick={() => save.mutate()}>
            {t("bookImport")}
          </LayerDialog.Action>
        </LayerDialog.Actions>
      </LayerDialog.Content>
    </LayerDialog>
  );
}

function TrashDialog({ book, onClose }: { book: Book; onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toasts = useKumoToastManager();
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState<Record<string, string>>({});
  const [permanent, setPermanent] = useState<BookPeer | "all" | null>(null);
  const base = `/api/ab/trash/${encodeURIComponent(book.guid)}`;
  const list = useQuery({
    queryKey: [...bookKey, book.guid, "trash", search, page],
    queryFn: () => apiGet<Page<BookPeer>>(base, { ...search, page, pageSize }),
  });
  const invalidate = () => {
    setPage(1);
    void qc.invalidateQueries({ queryKey: [...bookKey, book.guid] });
  };
  const restore = useMutation({
    mutationFn: (peer: BookPeer) => apiPost(`/api/ab/trash/restore/${encodeURIComponent(book.guid)}`, { id: peer.row_id }),
    onSuccess: () => {
      invalidate();
      toasts.add({ title: t("operationSuccess"), variant: "success" });
    },
  });
  const purge = useMutation({
    mutationFn: (target: BookPeer | "all") => remove(base, { id: target === "all" ? null : target.row_id }),
    onSuccess: () => { setPermanent(null); invalidate(); },
  });
  return (
    <>
      <Dialog.Root open onOpenChange={(open) => { if (!open && !permanent) onClose(); }}>
        <Dialog size="lg" className={resourceFormDialogPanelClass}>
          <DialogHeader title={t("bookTrash")} description={book.owner ? book.name : t("myAddressBook")} />
          <DialogBody>
            <div className="@container grid min-w-0 gap-4">
              <div className="grid gap-3 @lg:grid-cols-3">
                {peerFields.slice(0, 3).map(([field, key]) => (
                  <div key={field} className="min-w-0">
                    <Input className="w-full min-w-0" label={t(key)} value={search[field] ?? ""}
                      onChange={(event) => { setSearch({ ...search, [field]: event.target.value }); setPage(1); }} />
                  </div>
                ))}
              </div>
              {list.data?.data.length ? <DeviceTable trash rows={list.data.data} actions={(peer) => (
                <div className="flex gap-1">
                  <Button size="xs" variant="secondary" loading={restore.isPending && restore.variables?.row_id === peer.row_id}
                    disabled={restore.isPending} onClick={() => restore.mutate(peer)}>{t("bookRestore")}</Button>
                  <Button size="xs" variant="ghost" onClick={() => { purge.reset(); setPermanent(peer); }}>{t("delete")}</Button>
                </div>
              )} /> : null}
              {list.isPending && <TableState tone="loading">{t("loading")}</TableState>}
              {list.error && <TableState tone="error">{message(list.error, t("operationFailed"))}</TableState>}
              {list.data?.total === 0 && <TableState tone="empty">{t("noData")}</TableState>}
              {list.data && (page > 1 || list.data.total > pageSize) && <Pager page={page} total={list.data.total} setPage={setPage} />}
            </div>
          </DialogBody>
          <DialogFooter error={restore.error ? message(restore.error, t("operationFailed")) : undefined}>
            <Button variant="secondary" onClick={onClose}>{t("close")}</Button>
            <Button variant="destructive" disabled={!list.data?.total && !Object.values(search).some(Boolean)}
              onClick={() => { purge.reset(); setPermanent("all"); }}>{t("bookEmptyTrash")}</Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>
      {permanent && (
        <ConfirmDialog open title={t(permanent === "all" ? "bookEmptyTrash" : "confirmDeleteTitle")}
          description={t("bookPurgeDescription")} confirmLabel={t("delete")} cancelLabel={t("cancel")}
          loading={purge.isPending} error={purge.error ? message(purge.error, t("operationFailed")) : undefined}
          onOpenChange={(open) => { if (!open) setPermanent(null); }} onConfirm={() => purge.mutate(permanent)} />
      )}
    </>
  );
}

function GrantDialog({ book, onClose }: { book: Book; onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [page, setPage] = useState(1);
  const [type, setType] = useState<Grant["type"]>("user");
  const [target, setTarget] = useState("");
  const [level, setLevel] = useState(1);
  const [editing, setEditing] = useState<Grant>();
  const [revoking, setRevoking] = useState<Grant>();
  const rules = useQuery({
    queryKey: [...bookKey, book.guid, "grants", page],
    queryFn: () => apiGet<Page<Grant>>("/api/ab/rules", { ab: book.guid, page, pageSize }),
  });
  const invalidate = () => void qc.invalidateQueries({ queryKey: bookKey });
  const save = useMutation({
    mutationFn: () => {
      if (editing) return apiPatch("/api/ab/rule", { guid: editing.guid, rule: level });
      return apiPost("/api/ab/rule", {
        guid: book.guid, rule: level, user: type === "user" ? target.trim() : "",
        group: type === "group" ? target.trim() : "",
      });
    },
    onSuccess: () => { setEditing(undefined); setTarget(""); setLevel(1); invalidate(); },
  });
  const revoke = useMutation({
    mutationFn: (rule: Grant) => remove("/api/ab/rules", [rule.guid]),
    onSuccess: () => { setRevoking(undefined); setPage(1); invalidate(); },
  });
  const grantName = (grant: Grant) => grant.type === "everyone" ? t("bookEveryone") : grant.user || grant.group || "—";
  return (
    <>
      <Dialog.Root open onOpenChange={(open) => { if (!open && !revoking) onClose(); }}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader title={t("shareRules")} description={book.name} />
          <DialogBody>
            <Table>
              <Table.Header><Table.Row>
                {["type", "name", "rule", "actions"].map((key) => <Table.Head key={key}>{t(key)}</Table.Head>)}
              </Table.Row></Table.Header>
              <Table.Body>
                {rules.data?.data.map((grant) => (
                  <Table.Row key={grant.guid}>
                    <Table.Cell>{t(grant.type === "group" ? "ruleGroup" : grant.type === "user" ? "rulePersonal" : "bookEveryone")}</Table.Cell>
                    <Table.Cell>{grantName(grant)}</Table.Cell>
                    <Table.Cell><Badge>{t(permissionLabels[grant.rule] || "noData")}</Badge></Table.Cell>
                    <Table.Cell><div className="flex gap-1">
                      <Button size="xs" variant="ghost" onClick={() => {
                        save.reset(); setEditing(grant); setType(grant.type); setTarget(grantName(grant)); setLevel(grant.rule);
                      }}>{t("edit")}</Button>
                      <Button size="xs" variant="ghost" onClick={() => { revoke.reset(); setRevoking(grant); }}>
                        {t("bookRevoke")}
                      </Button>
                    </div></Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table>
            {rules.isPending && <TableState tone="loading">{t("loading")}</TableState>}
            {rules.error && <TableState tone="error">{message(rules.error, t("operationFailed"))}</TableState>}
            {rules.data?.total === 0 && <TableState tone="empty">{t("noData")}</TableState>}
            {rules.data && <Pager page={page} total={rules.data.total} setPage={setPage} />}
            <form className="mt-4 grid items-end gap-3 border-t border-kumo-line pt-4 sm:grid-cols-2"
              onSubmit={(event) => { event.preventDefault(); if (editing || target.trim() || type === "everyone") save.mutate(); }}>
              <Radio.Group<Grant["type"]> legend={t("type")} value={type} disabled={Boolean(editing)}
                orientation="horizontal" className="sm:col-span-2"
                onValueChange={(value) => { setType(value); setTarget(""); }}>
                <Radio.Item label={t("rulePersonal")} value="user" />
                <Radio.Item label={t("ruleGroup")} value="group" />
                <Radio.Item label={t("bookEveryone")} value="everyone" />
              </Radio.Group>
              {type !== "everyone" && <Input label={t(type === "user" ? "username" : "bookGroupName")}
                value={target} disabled={Boolean(editing)} required maxLength={120} onChange={(event) => setTarget(event.target.value)} />}
              <Radio.Group<number> legend={t("rule")} value={level} orientation="horizontal"
                className="sm:col-span-2" onValueChange={setLevel}>
                {[1, 2, 3].map((rule) => <Radio.Item<number> key={rule} value={rule} label={t(permissionLabels[rule])} />)}
              </Radio.Group>
              <div className="flex justify-end gap-2 sm:col-span-2">
                {editing && <Button type="button" variant="secondary" onClick={() => {
                  setEditing(undefined); setType("user"); setTarget(""); setLevel(1);
                }}>{t("cancel")}</Button>}
                <Button type="submit" loading={save.isPending}>{t(editing ? "save" : "bookGrantAccess")}</Button>
              </div>
              {save.error && <p role="alert" className="text-sm text-kumo-danger sm:col-span-2">{message(save.error, t("operationFailed"))}</p>}
            </form>
          </DialogBody>
          <DialogFooter><Button variant="secondary" onClick={onClose}>{t("close")}</Button></DialogFooter>
        </Dialog>
      </Dialog.Root>
      {revoking && (
        <ConfirmDialog open title={t("bookRevoke")} description={t("bookRevokeDescription", { name: grantName(revoking) })}
          confirmLabel={t("bookRevoke")} cancelLabel={t("cancel")} loading={revoke.isPending}
          error={revoke.error ? message(revoke.error, t("operationFailed")) : undefined}
          onOpenChange={(open) => { if (!open) setRevoking(undefined); }} onConfirm={() => revoke.mutate(revoking)} />
      )}
    </>
  );
}
