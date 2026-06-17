import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import {
  Bell,
  CheckCircle,
  EnvelopeSimple,
  Megaphone,
  PaperPlaneTilt,
  Trash,
} from "@phosphor-icons/react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPost, ApiError } from "../lib/api";
import { usePublicAdminConfig } from "../lib/adminTitle";
import { formatDateTime } from "../lib/dateFormat";

const PAGE_SIZE = 10;

type MessageKind = "announcement" | "broadcast" | "private";
type MessageFolder = "inbox" | "announcements" | "private" | "sent" | "management";

interface ListResult<T> {
  list: T[];
  total: number;
  page: number;
  page_size: number;
}

interface CurrentUser {
  username: string;
  route_names?: string[];
}

interface Message {
  id: number;
  sender_id: number;
  sender_name: string;
  recipient_id: number;
  recipient_name: string;
  kind: MessageKind;
  title: string;
  body: string;
  status: number;
  is_read: boolean;
  created_at?: string;
}

interface MessageUser {
  id: number;
  username: string;
  nickname: string;
  email: string;
}

interface ComposeForm {
  kind: MessageKind;
  recipient_id: number;
  title: string;
  body: string;
}

const initialCompose: ComposeForm = {
  kind: "private",
  recipient_id: 0,
  title: "",
  body: "",
};

export function MessageCenterPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const adminConfig = usePublicAdminConfig();
  const displayTimeZone = adminConfig.data?.timezone?.trim() || undefined;
  const [folder, setFolder] = useState<MessageFolder>("inbox");
  const [page, setPage] = useState(1);
  const [composeOpen, setComposeOpen] = useState(false);
  const [compose, setCompose] = useState<ComposeForm>(initialCompose);
  const [recipientSearch, setRecipientSearch] = useState("");
  const [formError, setFormError] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<Message | null>(null);

  const currentUser = useQuery({
    queryKey: ["current-user"],
    queryFn: () => apiGet<CurrentUser>("/api/admin/user/current"),
  });
  const isAdmin = currentUser.data?.route_names?.includes("*") ?? false;
  const effectiveFolder = !isAdmin && folder === "management" ? "inbox" : folder;

  const messages = useQuery({
    queryKey: ["messages", effectiveFolder, page],
    queryFn: () => {
      if (effectiveFolder === "management") {
        return apiGet<ListResult<Message>>("/api/admin/message/list", {
          page,
          page_size: PAGE_SIZE,
        });
      }
      return apiGet<ListResult<Message>>("/api/admin/my/message/list", {
        page,
        page_size: PAGE_SIZE,
        folder: effectiveFolder === "inbox" ? "" : effectiveFolder,
      });
    },
  });

  const users = useQuery({
    queryKey: ["message-users", recipientSearch],
    enabled: composeOpen && compose.kind === "private",
    queryFn: () =>
      apiGet<ListResult<MessageUser>>("/api/admin/my/message/users", {
        page: 1,
        page_size: 50,
        q: recipientSearch,
      }),
  });

  const unread = useQuery({
    queryKey: ["message-unread"],
    queryFn: () => apiGet<{ unread: number }>("/api/admin/my/message/unread"),
  });

  const folders = useMemo(() => {
    const base: { key: MessageFolder; label: string }[] = [
      { key: "inbox", label: "messageInbox" },
      { key: "announcements", label: "messageAnnouncements" },
      { key: "private", label: "messagePrivate" },
      { key: "sent", label: "messageSent" },
    ];
    if (isAdmin) base.push({ key: "management", label: "messageManagement" });
    return base;
  }, [isAdmin]);

  const save = useMutation({
    mutationFn: () => {
      const title = compose.title.trim();
      const body = compose.body.trim();
      if (!title || !body) {
        throw new ApiError(101, t("messageTitleBodyRequired"));
      }
      if (compose.kind === "private" && compose.recipient_id <= 0) {
        throw new ApiError(101, t("messageRecipientRequired"));
      }
      const payload = { ...compose, title, body };
      if (isAdmin) return apiPost("/api/admin/message/create", payload);
      return apiPost("/api/admin/my/message/create", payload);
    },
    onSuccess: () => {
      setComposeOpen(false);
      setCompose(initialCompose);
      setFormError("");
      void qc.invalidateQueries({ queryKey: ["messages"] });
      void qc.invalidateQueries({ queryKey: ["message-unread"] });
    },
    onError: (err) => setFormError((err as Error).message || t("operationFailed")),
  });

  const markRead = useMutation({
    mutationFn: (id: number) => apiPost("/api/admin/my/message/read", { id }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["messages"] });
      void qc.invalidateQueries({ queryKey: ["message-unread"] });
    },
  });

  const remove = useMutation({
    mutationFn: (message: Message) =>
      apiPost(
        effectiveFolder === "management"
          ? "/api/admin/message/delete"
          : "/api/admin/my/message/delete",
        { id: message.id },
      ),
    onSuccess: () => {
      setDeleteTarget(null);
      void qc.invalidateQueries({ queryKey: ["messages"] });
      void qc.invalidateQueries({ queryKey: ["message-unread"] });
    },
  });

  const rows = messages.data?.list ?? [];
  const total = messages.data?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));

  const openCompose = () => {
    setCompose(isAdmin ? { ...initialCompose, kind: "announcement" } : initialCompose);
    setRecipientSearch("");
    setFormError("");
    setComposeOpen(true);
  };

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("messageCenter")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("messageCenterHint")}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge>
            {t("messageUnreadCount", { count: unread.data?.unread ?? 0 })}
          </Badge>
          <Button onClick={openCompose}>
            <PaperPlaneTilt size={16} />
            {t("messageCompose")}
          </Button>
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        {folders.map((item) => (
          <Button
            key={item.key}
            size="sm"
            variant={effectiveFolder === item.key ? "primary" : "secondary"}
            onClick={() => {
              setFolder(item.key);
              setPage(1);
            }}
          >
            {t(item.label)}
          </Button>
        ))}
      </div>

      <section className="rounded-lg border border-kumo-line bg-kumo-elevated">
        <div className="divide-y divide-kumo-line">
          {rows.map((message) => (
            <MessageRow
              key={message.id}
              message={message}
              management={effectiveFolder === "management"}
              timeZone={displayTimeZone}
              onRead={() => markRead.mutate(message.id)}
              onDelete={() => {
                remove.reset();
                setDeleteTarget(message);
              }}
            />
          ))}
        </div>
        {messages.isLoading && (
          <TableState tone="loading">{t("loading")}</TableState>
        )}
        {messages.error && (
          <TableState tone="error">
            {(messages.error as Error).message || t("operationFailed")}
          </TableState>
        )}
        {!messages.isLoading && !messages.error && rows.length === 0 && (
          <TableState tone="empty">{t("messageEmpty")}</TableState>
        )}
      </section>

      <div className="flex items-center justify-end gap-3 text-sm">
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

      <Dialog.Root open={composeOpen} onOpenChange={setComposeOpen}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader
            title={t("messageCompose")}
            description={t("messageComposeHint")}
          />
          <DialogBody>
            <div className="grid gap-4">
              {isAdmin && (
                <label className="block">
                  <span className="mb-1.5 block text-sm font-medium">
                    {t("messageType")}
                  </span>
                  <select
                    className="h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                    value={compose.kind}
                    onChange={(e) =>
                      setCompose((state) => ({
                        ...state,
                        kind: e.target.value as MessageKind,
                        recipient_id:
                          e.target.value === "private" ? state.recipient_id : 0,
                      }))
                    }
                  >
                    <option value="announcement">{t("messageAnnouncement")}</option>
                    <option value="broadcast">{t("messageBroadcast")}</option>
                    <option value="private">{t("messagePrivateOne")}</option>
                  </select>
                </label>
              )}

              {compose.kind === "private" && (
                <div className="rounded-lg border border-kumo-line bg-kumo-base p-3">
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("messageRecipient")}
                    </span>
                    <Input
                      aria-label={t("messageRecipientSearch")}
                      value={recipientSearch}
                      placeholder={t("messageRecipientSearch")}
                      onChange={(e) => setRecipientSearch(e.target.value)}
                    />
                  </label>
                  <select
                    className="mt-3 h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                    value={compose.recipient_id ? String(compose.recipient_id) : ""}
                    onChange={(e) =>
                      setCompose((state) => ({
                        ...state,
                        recipient_id: Number(e.target.value) || 0,
                      }))
                    }
                  >
                    <option value="">{t("selectResource")}</option>
                    {(users.data?.list ?? []).map((user) => (
                      <option key={user.id} value={String(user.id)}>
                        {userLabel(user)}
                      </option>
                    ))}
                  </select>
                  <span className="mt-1.5 block text-xs text-kumo-subtle">
                    {users.isLoading
                      ? t("loading")
                      : users.error
                        ? (users.error as Error).message || t("operationFailed")
                        : t("messageRecipientHint")}
                  </span>
                </div>
              )}

              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("messageTitle")}
                </span>
                <Input
                  aria-label={t("messageTitle")}
                  value={compose.title}
                  maxLength={120}
                  onChange={(e) =>
                    setCompose((state) => ({ ...state, title: e.target.value }))
                  }
                />
              </label>
              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("messageBody")}
                </span>
                <textarea
                  className="min-h-36 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                  value={compose.body}
                  maxLength={5000}
                  onChange={(e) =>
                    setCompose((state) => ({ ...state, body: e.target.value }))
                  }
                />
                <span className="mt-1.5 block text-xs text-kumo-subtle">
                  {t("messageBodyHint")}
                </span>
              </label>
              {formError && <InlineMessage tone="error">{formError}</InlineMessage>}
            </div>
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={() => setComposeOpen(false)}>
              {t("cancel")}
            </Button>
            <Button loading={save.isPending} onClick={() => save.mutate()}>
              {t("send")}
            </Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("messageDeleteTitle")}
        description={
          effectiveFolder === "management"
            ? t("messageDeleteGlobalDescription")
            : t("messageDeleteDescription")
        }
        confirmLabel={t("delete")}
        cancelLabel={t("cancel")}
        loading={remove.isPending}
        error={
          remove.error
            ? (remove.error as Error).message || t("operationFailed")
            : undefined
        }
        onOpenChange={(next) => {
          if (!next) {
            setDeleteTarget(null);
            remove.reset();
          }
        }}
        onConfirm={() => {
          if (deleteTarget) remove.mutate(deleteTarget);
        }}
      />
    </div>
  );
}

function MessageRow({
  message,
  management,
  timeZone,
  onRead,
  onDelete,
}: {
  message: Message;
  management: boolean;
  timeZone?: string;
  onRead: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const Icon =
    message.kind === "announcement"
      ? Bell
      : message.kind === "broadcast"
        ? Megaphone
        : EnvelopeSimple;
  const actor =
    message.kind === "private" && message.recipient_name
      ? `${message.sender_name || "—"} -> ${message.recipient_name}`
      : message.sender_name || t("systemMessage");

  return (
    <article className="p-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex min-w-0 gap-3">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
            <Icon size={19} />
          </div>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="break-words text-sm font-semibold">
                {message.title}
              </h2>
              <Badge>{t(messageKindKey(message.kind))}</Badge>
              {!management && !message.is_read && (
                <span className="rounded border border-kumo-brand/30 bg-kumo-tint px-2 py-0.5 text-xs text-kumo-default">
                  {t("messageUnread")}
                </span>
              )}
            </div>
            <p className="mt-1 text-xs text-kumo-subtle">
              {actor} · {formatDateTime(message.created_at, timeZone)}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
          {!management && !message.is_read && (
            <Button size="sm" variant="secondary" onClick={onRead}>
              <CheckCircle size={16} />
              {t("markRead")}
            </Button>
          )}
          <Button size="sm" variant="secondary-destructive" onClick={onDelete}>
            <Trash size={16} />
            {t("delete")}
          </Button>
        </div>
      </div>
      <p className="mt-3 whitespace-pre-wrap break-words rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm leading-6">
        {message.body}
      </p>
    </article>
  );
}

function messageKindKey(kind: MessageKind) {
  if (kind === "announcement") return "messageAnnouncement";
  if (kind === "broadcast") return "messageBroadcast";
  return "messagePrivateOne";
}

function userLabel(user: MessageUser) {
  const name = user.nickname || user.username;
  const suffix = user.email ? ` · ${user.email}` : "";
  return `${name} (#${user.id})${suffix}`;
}
