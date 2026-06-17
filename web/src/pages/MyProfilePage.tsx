import { Fragment, useState } from "react";
import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { cn } from "@cloudflare/kumo/utils";
import {
  CheckCircle,
  EnvelopeSimple,
  IdentificationBadge,
  Key,
  LockKey,
  LinkSimple,
  ShieldCheck,
  UserCircle,
  WarningCircle,
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
import { clearToken } from "../lib/auth";

interface CurrentUser {
  username: string;
  email: string;
  nickname: string;
  must_change_password: boolean;
}

interface AdminConfig {
  title?: string;
  hello?: string;
}

interface OAuthStatus {
  op: string;
  status: number;
}

interface BindStart {
  code: string;
  url: string;
}

export function MyProfilePage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [pwdMessage, setPwdMessage] = useState("");
  const [pwdError, setPwdError] = useState("");
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [oauthError, setOauthError] = useState("");
  const [bindingTarget, setBindingTarget] = useState("");
  const [unbindTarget, setUnbindTarget] = useState("");

  const user = useQuery({
    queryKey: ["current-user"],
    queryFn: () => apiGet<CurrentUser>("/api/admin/user/current"),
  });
  const config = useQuery({
    queryKey: ["admin-config"],
    queryFn: () => apiGet<AdminConfig>("/api/admin/config/admin"),
  });
  const oauth = useQuery({
    queryKey: ["my-oauth"],
    queryFn: () => apiPost<OAuthStatus[]>("/api/admin/user/myOauth"),
  });

  const changePwd = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/user/changeCurPwd", {
        old_password: oldPassword,
        new_password: newPassword,
      }),
    onSuccess: () => {
      setOldPassword("");
      setNewPassword("");
      setConfirmPassword("");
      setPwdMessage(t("passwordUpdatedLoginAgain"));
      clearToken();
      qc.clear();
      navigate("/login", {
        replace: true,
        state: { message: t("passwordUpdatedLoginAgain") },
      });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setPwdError(ae.message || t("operationFailed"));
    },
  });

  const bindOauth = async (op: string) => {
    setOauthError("");
    setBindingTarget(op);
    try {
      const res = await apiPost<BindStart>("/api/admin/oauth/bind", { op });
      window.open(res.url, "_blank", "noopener,noreferrer");
    } catch (err) {
      setOauthError((err as ApiError).message || t("operationFailed"));
    } finally {
      setBindingTarget("");
    }
  };

  const unbind = useMutation({
    mutationFn: (op: string) => apiPost("/api/admin/oauth/unbind", { op }),
    onSuccess: () => {
      setUnbindTarget("");
      void qc.invalidateQueries({ queryKey: ["my-oauth"] });
    },
  });

  const submitPassword = (e: React.FormEvent) => {
    e.preventDefault();
    setPwdMessage("");
    setPwdError("");
    if (!oldPassword || !newPassword || !confirmPassword) {
      setPwdError(t("passwordRequired"));
      return;
    }
    if (newPassword.length < 4) {
      setPwdError(t("passwordMinLength"));
      return;
    }
    if (newPassword !== confirmPassword) {
      setPwdError(t("passwordMismatch"));
      return;
    }
    changePwd.mutate();
  };

  const closePasswordDialog = () => {
    setPasswordOpen(false);
    setOldPassword("");
    setNewPassword("");
    setConfirmPassword("");
    setPwdMessage("");
    setPwdError("");
  };

  const currentUser = user.data;
  const boundCount = (oauth.data ?? []).filter((row) => row.status === 1).length;
  const passwordScoreValue = passwordScore(newPassword);

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("myInfo")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("myInfoHint")}
          </p>
        </div>
        {currentUser?.must_change_password && (
          <InlineMessage tone="error" className="lg:max-w-md">
            {t("passwordChangeRequired")}
          </InlineMessage>
        )}
      </div>

      <div className="space-y-5">
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div className="flex min-w-0 items-center gap-3">
              <div className="flex size-12 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <UserCircle size={28} weight="duotone" />
              </div>
              <div className="min-w-0">
                <h2 className="truncate text-base font-semibold">
                  {currentUser?.nickname || currentUser?.username || t("account")}
                </h2>
                <p className="mt-1 truncate text-sm text-kumo-subtle">
                  {currentUser?.username || (user.isLoading ? t("loading") : "—")}
                </p>
              </div>
            </div>
            <div className="flex shrink-0 flex-wrap items-center gap-2">
              <StatusChip tone={currentUser?.must_change_password ? "warning" : "success"}>
                {currentUser?.must_change_password
                  ? t("passwordChangeRequired")
                  : t("active")}
              </StatusChip>
              <Button
                variant="secondary"
                onClick={() => {
                  setPwdError("");
                  setPwdMessage("");
                  setPasswordOpen(true);
                }}
              >
                <LockKey size={16} />
                {t("changePassword")}
              </Button>
            </div>
          </div>

          <dl className="mt-5 grid gap-4 border-t border-kumo-line pt-4 text-sm md:grid-cols-3">
            <ProfileField
              icon={<IdentificationBadge size={18} />}
              label={t("username")}
              value={currentUser?.username}
              loading={user.isLoading}
            />
            <ProfileField
              icon={<EnvelopeSimple size={18} />}
              label={t("email")}
              value={currentUser?.email}
              loading={user.isLoading}
            />
            <ProfileField
              icon={<ShieldCheck size={18} />}
              label={t("oauthBinding")}
              value={t("boundSummary", {
                bound: boundCount,
                total: oauth.data?.length ?? 0,
              })}
              loading={oauth.isLoading}
            />
          </dl>
        </section>

        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="mb-4 flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <h2 className="text-base font-semibold">{t("oauthBinding")}</h2>
              <p className="mt-1 text-sm text-kumo-subtle">
                {t("oauthBindingHint")}
              </p>
            </div>
            <StatusChip>
              {t("boundSummary", {
                bound: boundCount,
                total: oauth.data?.length ?? 0,
              })}
            </StatusChip>
          </div>
          {oauthError && (
            <InlineMessage tone="error" className="mb-3">
              {oauthError}
            </InlineMessage>
          )}
          <div className="overflow-x-auto rounded-lg border border-kumo-line">
            <Table>
              <Table.Header>
                <Table.Row>
                  <Table.Head>{t("op")}</Table.Head>
                  <Table.Head>{t("status")}</Table.Head>
                  <Table.Head>{t("actions")}</Table.Head>
                </Table.Row>
              </Table.Header>
              <Table.Body>
                {(oauth.data ?? []).map((row) => (
                  <Table.Row key={row.op}>
                    <Table.Cell>
                      <div className="flex min-w-36 items-center gap-2">
                        <div className="flex size-8 shrink-0 items-center justify-center rounded-md border border-kumo-line bg-kumo-base text-kumo-subtle">
                          <LinkSimple size={16} />
                        </div>
                        <span className="font-medium">{row.op}</span>
                      </div>
                    </Table.Cell>
                    <Table.Cell>
                      <span
                        className={cn(
                          "inline-flex min-h-7 items-center gap-1.5 rounded-md border px-2.5 text-xs font-medium",
                          row.status === 1
                            ? "border-kumo-success/25 bg-kumo-success-tint/60 text-kumo-success"
                            : "border-kumo-line bg-kumo-base text-kumo-subtle",
                        )}
                      >
                        {row.status === 1 ? (
                          <CheckCircle size={14} weight="fill" />
                        ) : (
                          <WarningCircle size={14} />
                        )}
                        {row.status === 1 ? t("hasBind") : t("noBind")}
                      </span>
                    </Table.Cell>
                    <Table.Cell>
                      {row.status === 1 ? (
                        <Button
                          size="sm"
                          variant="secondary-destructive"
                          onClick={() => {
                            unbind.reset();
                            setUnbindTarget(row.op);
                          }}
                        >
                          {t("unbind")}
                        </Button>
                      ) : (
                        <Button
                          size="sm"
                          variant="ghost"
                          loading={bindingTarget === row.op}
                          disabled={Boolean(bindingTarget)}
                          onClick={() => void bindOauth(row.op)}
                        >
                          {t("toBind")}
                        </Button>
                      )}
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table>
            {oauth.isLoading && (
              <TableState tone="loading">{t("loading")}</TableState>
            )}
            {oauth.error && (
              <TableState tone="error">
                {(oauth.error as Error).message || t("operationFailed")}
              </TableState>
            )}
            {!oauth.isLoading && !oauth.error && (oauth.data ?? []).length === 0 && (
              <TableState tone="empty">{t("noData")}</TableState>
            )}
          </div>
        </section>

        {config.data?.hello && (
          <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
            <div className="mb-4 flex items-start gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <Key size={18} />
              </div>
              <div>
                <h2 className="text-base font-semibold">
                  {config.data.title || t("notice")}
                </h2>
                <p className="mt-1 text-sm text-kumo-subtle">{t("noticeHint")}</p>
              </div>
            </div>
            <NoticeRenderer text={config.data.hello} />
          </section>
        )}
      </div>

      <Dialog.Root
        open={passwordOpen}
        onOpenChange={(next) => {
          if (next) setPasswordOpen(true);
          else closePasswordDialog();
        }}
      >
        <Dialog size="sm" className={dialogPanelClass}>
          <DialogHeader
            title={t("changePassword")}
            description={t("changePasswordHint")}
          />
          <form onSubmit={submitPassword}>
            <DialogBody>
              <div className="grid gap-4">
                <label className="block">
                  <span className="mb-1.5 block text-sm font-medium">
                    {t("oldPassword")}
                  </span>
                  <Input
                    aria-label={t("oldPassword")}
                    type="password"
                    value={oldPassword}
                    autoComplete="current-password"
                    onChange={(e) => {
                      setOldPassword(e.target.value);
                      setPwdError("");
                    }}
                  />
                </label>
                <label className="block">
                  <span className="mb-1.5 block text-sm font-medium">
                    {t("newPassword")}
                  </span>
                  <Input
                    aria-label={t("newPassword")}
                    type="password"
                    value={newPassword}
                    autoComplete="new-password"
                    onChange={(e) => {
                      setNewPassword(e.target.value);
                      setPwdError("");
                    }}
                  />
                  <div className="mt-2" aria-hidden="true">
                    <div className="grid grid-cols-4 gap-1">
                      {[0, 1, 2, 3].map((step) => (
                        <span
                          key={step}
                          className={cn(
                            "h-1 rounded-full",
                            passwordScoreValue > step
                              ? "bg-kumo-brand"
                              : "bg-kumo-line",
                          )}
                        />
                      ))}
                    </div>
                  </div>
                  <span className="mt-1.5 block text-xs text-kumo-subtle">
                    {newPassword
                      ? t(passwordStrengthKey(passwordScoreValue))
                      : t("passwordMinLength")}
                  </span>
                </label>
                <label className="block">
                  <span className="mb-1.5 block text-sm font-medium">
                    {t("confirmPassword")}
                  </span>
                  <Input
                    aria-label={t("confirmPassword")}
                    type="password"
                    value={confirmPassword}
                    autoComplete="new-password"
                    onChange={(e) => {
                      setConfirmPassword(e.target.value);
                      setPwdError("");
                    }}
                  />
                </label>
                {pwdMessage && (
                  <InlineMessage tone="success">{pwdMessage}</InlineMessage>
                )}
                {pwdError && <InlineMessage tone="error">{pwdError}</InlineMessage>}
              </div>
            </DialogBody>
            <DialogFooter>
              <Button variant="secondary" type="button" onClick={closePasswordDialog}>
                {t("cancel")}
              </Button>
              <Button
                type="submit"
                disabled={changePwd.isPending}
                loading={changePwd.isPending}
              >
                {t("save")}
              </Button>
            </DialogFooter>
          </form>
        </Dialog>
      </Dialog.Root>

      <ConfirmDialog
        open={Boolean(unbindTarget)}
        title={t("confirmUnbindTitle")}
        description={t("confirmUnbindDescription")}
        confirmLabel={t("unbind")}
        cancelLabel={t("cancel")}
        error={
          unbind.error
            ? (unbind.error as Error).message || t("operationFailed")
            : undefined
        }
        loading={unbind.isPending}
        onOpenChange={(next) => {
          if (!next) {
            setUnbindTarget("");
            unbind.reset();
          }
        }}
        onConfirm={() => {
          if (unbindTarget) unbind.mutate(unbindTarget);
        }}
      />
    </div>
  );
}

function ProfileField({
  icon,
  label,
  value,
  loading,
}: {
  icon: ReactNode;
  label: string;
  value?: string;
  loading?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="flex items-center gap-2 text-xs font-medium text-kumo-subtle">
        {icon}
        {label}
      </dt>
      <dd className="mt-2 min-h-5 break-words text-sm font-medium">
        {loading ? "…" : value || "—"}
      </dd>
    </div>
  );
}

function StatusChip({
  children,
  tone = "default",
}: {
  children: ReactNode;
  tone?: "default" | "success" | "warning";
}) {
  return (
    <span
      className={cn(
        "inline-flex min-h-7 shrink-0 items-center rounded-md border px-2.5 text-xs font-medium",
        tone === "success" &&
          "border-kumo-success/25 bg-kumo-success-tint/60 text-kumo-success",
        tone === "warning" &&
          "border-kumo-warning/25 bg-kumo-warning-tint/60 text-kumo-warning",
        tone === "default" && "border-kumo-line bg-kumo-base text-kumo-subtle",
      )}
    >
      {children}
    </span>
  );
}

function passwordScore(password: string): number {
  if (!password) return 0;
  let score = password.length >= 4 ? 1 : 0;
  if (password.length >= 8) score += 1;
  if (/[A-Z]/.test(password) && /[a-z]/.test(password)) score += 1;
  if (/\d/.test(password) || /[^A-Za-z0-9]/.test(password)) score += 1;
  return Math.min(score, 4);
}

function passwordStrengthKey(score: number): string {
  if (score >= 4) return "passwordStrengthStrong";
  if (score >= 2) return "passwordStrengthMedium";
  return "passwordStrengthWeak";
}

function NoticeRenderer({ text }: { text: string }) {
  const rows = text
    .split(/\r?\n/)
    .map((line, index) => ({ line: line.trim(), index }))
    .filter(({ line }) => line.length > 0);

  if (rows.length === 0) {
    return <p className="text-sm text-kumo-subtle">—</p>;
  }

  return (
    <div className="space-y-3 break-words text-sm leading-6">
      {rows.map(({ line, index }) => {
        if (line.startsWith("#### ")) {
          return (
            <h3 key={index} className="text-base font-semibold">
              {renderInlineMarkdown(line.slice(5))}
            </h3>
          );
        }
        if (line.startsWith("### ")) {
          return (
            <h3 key={index} className="text-base font-semibold">
              {renderInlineMarkdown(line.slice(4))}
            </h3>
          );
        }
        if (line.startsWith("> ")) {
          return (
            <blockquote
              key={index}
              className="border-l-2 border-kumo-brand bg-kumo-base px-3 py-2 text-kumo-default"
            >
              {renderInlineMarkdown(line.slice(2))}
            </blockquote>
          );
        }
        if (line.startsWith("- ")) {
          return (
            <p key={index} className="flex min-w-0 gap-2 text-kumo-default">
              <span className="mt-2 size-1.5 shrink-0 rounded-full bg-kumo-brand" />
              <span>{renderInlineMarkdown(line.slice(2))}</span>
            </p>
          );
        }
        return <p key={index}>{renderInlineMarkdown(line)}</p>;
      })}
    </div>
  );
}

function renderInlineMarkdown(text: string): ReactNode {
  const parts: ReactNode[] = [];
  const pattern = /(\*\*[^*]+\*\*|\[[^\]]+\]\([^)]+\))/g;
  let cursor = 0;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > cursor) {
      parts.push(text.slice(cursor, match.index));
    }

    const token = match[0];
    if (token.startsWith("**")) {
      parts.push(
        <strong key={`${match.index}-strong`} className="font-semibold">
          {token.slice(2, -2)}
        </strong>,
      );
    } else {
      const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
      if (link && isSafeLink(link[2])) {
        parts.push(
          <a
            key={`${match.index}-link`}
            href={link[2]}
            target="_blank"
            rel="noreferrer"
            className="break-words font-medium text-kumo-brand underline-offset-4 hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
          >
            {link[1]}
          </a>,
        );
      } else {
        parts.push(token);
      }
    }

    cursor = match.index + token.length;
  }

  if (cursor < text.length) {
    parts.push(text.slice(cursor));
  }

  return (
    <>
      {parts.map((part, index) => (
        <Fragment key={index}>{part}</Fragment>
      ))}
    </>
  );
}

function isSafeLink(url: string): boolean {
  return url.startsWith("https://") || url.startsWith("http://") || url.startsWith("/");
}
