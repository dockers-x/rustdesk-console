import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Switch } from "@cloudflare/kumo/components/switch";
import { Badge } from "@cloudflare/kumo/components/badge";
import { apiGet, apiPost } from "../lib/api";

interface User {
  id: number;
  username: string;
  email: string;
  nickname: string;
  group_id: number;
  is_admin: boolean | null;
  status: number;
  created_at?: string;
}
interface UserList {
  list: User[];
  total: number;
  page: number;
  page_size: number;
}

interface FormState {
  id: number;
  username: string;
  email: string;
  nickname: string;
  group_id: number;
  is_admin: boolean;
  status: number;
  password: string;
}

const emptyForm: FormState = {
  id: 0,
  username: "",
  email: "",
  nickname: "",
  group_id: 1,
  is_admin: false,
  status: 1,
  password: "",
};

export function UsersPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState("");
  const pageSize = 10;

  const { data, isLoading } = useQuery({
    queryKey: ["users", page, search],
    queryFn: () =>
      apiGet<UserList>("/api/admin/user/list", {
        page,
        page_size: pageSize,
        username: search || undefined,
      }),
  });

  const [open, setOpen] = useState(false);
  const [form, setForm] = useState<FormState>(emptyForm);
  const editing = form.id !== 0;

  const openCreate = () => {
    setForm(emptyForm);
    setOpen(true);
  };
  const openEdit = (u: User) => {
    setForm({
      id: u.id,
      username: u.username,
      email: u.email,
      nickname: u.nickname,
      group_id: u.group_id,
      is_admin: !!u.is_admin,
      status: u.status,
      password: "",
    });
    setOpen(true);
  };

  const save = useMutation({
    mutationFn: async () => {
      const url = editing ? "/api/admin/user/update" : "/api/admin/user/create";
      await apiPost(url, form);
    },
    onSuccess: () => {
      setOpen(false);
      void qc.invalidateQueries({ queryKey: ["users"] });
    },
  });

  const remove = useMutation({
    mutationFn: (id: number) => apiPost("/api/admin/user/delete", { id }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["users"] }),
  });

  const total = data?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));

  return (
    <div>
      <div className="mb-4 flex items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">{t("users")}</h1>
        <div className="flex items-center gap-2">
          <Input
            placeholder={t("search")}
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              setPage(1);
            }}
          />
          <Button onClick={openCreate}>{t("create")}</Button>
        </div>
      </div>

      <div className="rounded-lg border border-color-border">
        <Table>
          <Table.Header>
            <Table.Row>
              <Table.Head>ID</Table.Head>
              <Table.Head>{t("username")}</Table.Head>
              <Table.Head>{t("email")}</Table.Head>
              <Table.Head>{t("nickname")}</Table.Head>
              <Table.Head>{t("isAdmin")}</Table.Head>
              <Table.Head>{t("status")}</Table.Head>
              <Table.Head>{t("actions")}</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {(data?.list ?? []).map((u) => (
              <Table.Row key={u.id}>
                <Table.Cell>{u.id}</Table.Cell>
                <Table.Cell>{u.username}</Table.Cell>
                <Table.Cell>{u.email}</Table.Cell>
                <Table.Cell>{u.nickname}</Table.Cell>
                <Table.Cell>
                  {u.is_admin ? <Badge>{t("isAdmin")}</Badge> : "—"}
                </Table.Cell>
                <Table.Cell>
                  {u.status === 1 ? t("enabled") : t("disabled")}
                </Table.Cell>
                <Table.Cell>
                  <div className="flex gap-2">
                    <Button size="sm" variant="ghost" onClick={() => openEdit(u)}>
                      {t("edit")}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => {
                        if (confirm(t("confirmDelete"))) remove.mutate(u.id);
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
        {isLoading && <div className="p-4 text-sm text-color-muted">…</div>}
        {!isLoading && (data?.list?.length ?? 0) === 0 && (
          <div className="p-4 text-sm text-color-muted">No data</div>
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

      <Dialog.Root open={open} onOpenChange={setOpen}>
        <Dialog>
          <Dialog.Title>{editing ? t("editUser") : t("newUser")}</Dialog.Title>
          <div className="mt-4 space-y-3">
            <Labeled label={t("username")}>
              <Input
                value={form.username}
                disabled={editing}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
              />
            </Labeled>
            {!editing && (
              <Labeled label={t("password")}>
                <Input
                  type="password"
                  value={form.password}
                  onChange={(e) =>
                    setForm({ ...form, password: e.target.value })
                  }
                />
              </Labeled>
            )}
            <Labeled label={t("email")}>
              <Input
                value={form.email}
                onChange={(e) => setForm({ ...form, email: e.target.value })}
              />
            </Labeled>
            <Labeled label={t("nickname")}>
              <Input
                value={form.nickname}
                onChange={(e) => setForm({ ...form, nickname: e.target.value })}
              />
            </Labeled>
            <Labeled label={t("groupId")}>
              <Input
                type="number"
                value={String(form.group_id)}
                onChange={(e) =>
                  setForm({ ...form, group_id: Number(e.target.value) || 0 })
                }
              />
            </Labeled>
            <div className="flex items-center justify-between">
              <span className="text-sm">{t("isAdmin")}</span>
              <Switch
                checked={form.is_admin}
                onCheckedChange={(v: boolean) =>
                  setForm({ ...form, is_admin: v })
                }
              />
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm">{t("status")}</span>
              <Switch
                checked={form.status === 1}
                onCheckedChange={(v: boolean) =>
                  setForm({ ...form, status: v ? 1 : 2 })
                }
              />
            </div>
          </div>
          <div className="mt-6 flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setOpen(false)}>
              {t("cancel")}
            </Button>
            <Button onClick={() => save.mutate()} disabled={save.isPending}>
              {t("save")}
            </Button>
          </div>
        </Dialog>
      </Dialog.Root>
    </div>
  );
}

function Labeled({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="mb-1 block text-sm">{label}</span>
      {children}
    </label>
  );
}
