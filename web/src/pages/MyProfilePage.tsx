import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { cn } from "@cloudflare/kumo/utils";
import {
  ArrowClockwise,
  Bell,
  CheckCircle,
  EnvelopeSimple,
  IdentificationBadge,
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

interface OAuthStatus {
  op: string;
  oauth_type?: string;
  status: number;
  name?: string;
  username?: string;
  email?: string;
  verified_email?: boolean;
  picture?: string;
  created_at?: string;
}

interface BindStart {
  code: string;
  url: string;
}

interface MessageSummary {
  id: number;
  title: string;
  body: string;
  sender_name: string;
  kind: string;
  is_read: boolean;
  created_at?: string;
}

interface MessageLatest {
  list: MessageSummary[];
  unread: number;
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
  const [oauthMessage, setOauthMessage] = useState("");
  const [bindingTarget, setBindingTarget] = useState("");
  const [refreshOnFocus, setRefreshOnFocus] = useState(false);
  const [unbindTarget, setUnbindTarget] = useState("");

  const user = useQuery({
    queryKey: ["current-user"],
    queryFn: () => apiGet<CurrentUser>("/api/admin/user/current"),
  });
  const oauth = useQuery({
    queryKey: ["my-oauth"],
    queryFn: () => apiPost<OAuthStatus[]>("/api/admin/user/myOauth"),
  });
  const latestMessages = useQuery({
    queryKey: ["profile-latest-messages"],
    queryFn: () => apiGet<MessageLatest>("/api/admin/my/message/latest"),
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
    setOauthMessage("");
    setBindingTarget(op);
    try {
      const res = await apiPost<BindStart>("/api/admin/oauth/bind", { op });
      window.open(res.url, "_blank", "noopener,noreferrer");
      setOauthMessage(t("oauthBindReturnHint"));
      setRefreshOnFocus(true);
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
      setOauthMessage("");
      void qc.invalidateQueries({ queryKey: ["my-oauth"] });
    },
  });

  useEffect(() => {
    if (!refreshOnFocus) return;
    const refresh = () => {
      setRefreshOnFocus(false);
      void qc.invalidateQueries({ queryKey: ["my-oauth"] });
    };
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [qc, refreshOnFocus]);

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
              label={t("loginMethods")}
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
              <h2 className="text-base font-semibold">{t("loginMethods")}</h2>
              <p className="mt-1 text-sm text-kumo-subtle">
                {t("loginMethodsHint")}
              </p>
            </div>
            <div className="flex shrink-0 flex-wrap items-center gap-2">
              <StatusChip>
                {t("boundSummary", {
                  bound: boundCount,
                  total: oauth.data?.length ?? 0,
                })}
              </StatusChip>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => {
                  setOauthMessage("");
                  void qc.invalidateQueries({ queryKey: ["my-oauth"] });
                }}
              >
                <ArrowClockwise size={16} />
                {t("refreshBindings")}
              </Button>
            </div>
          </div>
          <p className="mb-3 rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm leading-6 text-kumo-subtle">
            {t("oauthBindingHint")}
          </p>
          {oauthMessage && (
            <InlineMessage tone="success" className="mb-3">
              {oauthMessage}
            </InlineMessage>
          )}
          {oauthError && (
            <InlineMessage tone="error" className="mb-3">
              {oauthError}
            </InlineMessage>
          )}
          <div className="rounded-lg border border-kumo-line bg-kumo-base">
            <div className="grid gap-0 divide-y divide-kumo-line">
              {(oauth.data ?? []).map((row) => (
                <OAuthMethodRow
                  key={row.op}
                  row={row}
                  bindingTarget={bindingTarget}
                  unbindPending={unbind.isPending && unbindTarget === row.op}
                  onBind={() => void bindOauth(row.op)}
                  onUnbind={() => {
                    unbind.reset();
                    setUnbindTarget(row.op);
                  }}
                />
              ))}
            </div>
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

        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div className="flex items-start gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <Bell size={18} />
              </div>
              <div>
                <h2 className="text-base font-semibold">{t("latestMessages")}</h2>
                <p className="mt-1 text-sm text-kumo-subtle">
                  {t("latestMessagesHint")}
                </p>
              </div>
            </div>
            <Button size="sm" variant="secondary" onClick={() => navigate("/messages")}>
              {t("messageCenter")}
            </Button>
          </div>
          <div className="rounded-lg border border-kumo-line bg-kumo-base">
            <div className="divide-y divide-kumo-line">
              {(latestMessages.data?.list ?? []).map((message) => (
                <button
                  key={message.id}
                  type="button"
                  className="block w-full px-3 py-3 text-left transition-colors hover:bg-kumo-tint/60 focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                  onClick={() => navigate("/messages")}
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="break-words text-sm font-semibold">
                      {message.title}
                    </span>
                    {!message.is_read && (
                      <span className="rounded border border-kumo-brand/30 bg-kumo-tint px-2 py-0.5 text-xs">
                        {t("messageUnread")}
                      </span>
                    )}
                  </div>
                  <p className="mt-1 line-clamp-2 text-sm leading-6 text-kumo-subtle">
                    {message.body}
                  </p>
                </button>
              ))}
            </div>
            {latestMessages.isLoading && (
              <TableState tone="loading">{t("loading")}</TableState>
            )}
            {latestMessages.error && (
              <TableState tone="error">
                {(latestMessages.error as Error).message || t("operationFailed")}
              </TableState>
            )}
            {!latestMessages.isLoading &&
              !latestMessages.error &&
              (latestMessages.data?.list ?? []).length === 0 && (
                <TableState tone="empty">{t("messageEmpty")}</TableState>
              )}
          </div>
        </section>
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

function OAuthMethodRow({
  row,
  bindingTarget,
  unbindPending,
  onBind,
  onUnbind,
}: {
  row: OAuthStatus;
  bindingTarget: string;
  unbindPending: boolean;
  onBind: () => void;
  onUnbind: () => void;
}) {
  const { t } = useTranslation();
  const isBound = row.status === 1;
  const identity = row.name || row.username || row.email || row.op;
  const secondary = isBound
    ? t("oauthBoundIdentity", { identity })
    : t("oauthUnboundHint");

  return (
    <div className="flex flex-col gap-3 p-3 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex min-w-0 gap-3">
        <div className="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-kumo-line bg-kumo-elevated text-kumo-subtle">
          {row.picture ? (
            <img
              src={row.picture}
              alt=""
              className="size-full object-cover"
              referrerPolicy="no-referrer"
            />
          ) : (
            <LinkSimple size={18} />
          )}
        </div>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="break-all text-sm font-semibold">{row.op}</h3>
            <StatusChip>{(row.oauth_type || "oauth").toUpperCase()}</StatusChip>
            <span
              className={cn(
                "inline-flex min-h-7 items-center gap-1.5 rounded-md border px-2.5 text-xs font-medium",
                isBound
                  ? "border-kumo-success/25 bg-kumo-success-tint/60 text-kumo-success"
                  : "border-kumo-line bg-kumo-elevated text-kumo-subtle",
              )}
            >
              {isBound ? (
                <CheckCircle size={14} weight="fill" />
              ) : (
                <WarningCircle size={14} />
              )}
              {isBound ? t("hasBind") : t("noBind")}
            </span>
          </div>
          <p className="mt-1 break-words text-sm text-kumo-subtle">
            {secondary}
          </p>
          {isBound && (row.email || row.username) && (
            <div className="mt-2 flex flex-wrap gap-2 text-xs text-kumo-subtle">
              {row.email && (
                <span className="inline-flex min-h-7 items-center rounded-md border border-kumo-line bg-kumo-elevated px-2">
                  {row.email}
                </span>
              )}
              {row.username && row.username !== row.email && (
                <span className="inline-flex min-h-7 items-center rounded-md border border-kumo-line bg-kumo-elevated px-2">
                  {row.username}
                </span>
              )}
              {row.email && (
                <span className="inline-flex min-h-7 items-center rounded-md border border-kumo-line bg-kumo-elevated px-2">
                  {row.verified_email ? t("verifiedEmail") : t("unverifiedEmail")}
                </span>
              )}
            </div>
          )}
        </div>
      </div>
      <div className="flex shrink-0 sm:justify-end">
        {isBound ? (
          <Button
            size="sm"
            variant="secondary-destructive"
            disabled={unbindPending}
            onClick={onUnbind}
          >
            {t("unbind")}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="secondary"
            loading={bindingTarget === row.op}
            disabled={Boolean(bindingTarget)}
            onClick={onBind}
          >
            {t("toBind")}
          </Button>
        )}
      </div>
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
