import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as QRCode from "qrcode";
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
  WarningCircle,
} from "@phosphor-icons/react";
import { AvatarPreview } from "../components/AvatarPreview";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPost, http, ApiError } from "../lib/api";
import { clearToken } from "../lib/auth";
import { formatUnixSeconds } from "../lib/dateFormat";

interface CurrentUser {
  username: string;
  email: string;
  avatar: string;
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

interface SecurityStatus {
  tfa_enabled: boolean;
  tfa_enforced: boolean;
  email_verification_enabled: boolean;
  login_device_verification_enabled: boolean;
  system_require_totp?: boolean;
  system_require_email_verification?: boolean;
  system_require_device_verification?: boolean;
  system_allow_trusted_login_devices?: boolean;
  effective_tfa_required?: boolean;
  effective_email_verification_enabled?: boolean;
  effective_login_device_verification_enabled?: boolean;
  trusted_device_count: number;
}

interface TfaSetup {
  secret: string;
  uri: string;
}

interface TrustedLoginDevice {
  id: number;
  device_id: string;
  device_uuid: string;
  device_name: string;
  device_os: string;
  device_type: string;
  ip: string;
  last_seen_at: number;
}

export function MyProfilePage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const avatarInputRef = useRef<HTMLInputElement>(null);
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
  const [profileMessage, setProfileMessage] = useState("");
  const [profileError, setProfileError] = useState("");
  const [securityMessage, setSecurityMessage] = useState("");
  const [securityError, setSecurityError] = useState("");
  const [tfaSetup, setTfaSetup] = useState<TfaSetup | null>(null);
  const [tfaQrDataUrl, setTfaQrDataUrl] = useState("");
  const [tfaQrError, setTfaQrError] = useState("");
  const [tfaCode, setTfaCode] = useState("");
  const [tfaDisableCode, setTfaDisableCode] = useState("");
  const [trustedDeleteTarget, setTrustedDeleteTarget] =
    useState<TrustedLoginDevice | null>(null);

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
  const security = useQuery({
    queryKey: ["my-security"],
    queryFn: () => apiGet<SecurityStatus>("/api/admin/user/mySecurity"),
  });
  const trustedDevices = useQuery({
    queryKey: ["my-trusted-login-devices"],
    queryFn: () =>
      apiGet<TrustedLoginDevice[]>("/api/admin/user/myTrustedLoginDevices"),
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

  const uploadAvatar = useMutation({
    mutationFn: async (file: File) => {
      if (file.type && !file.type.startsWith("image/")) {
        throw new Error(t("avatarImageOnly"));
      }
      const formData = new FormData();
      formData.append("file", file);
      const uploaded = (await http.post(
        "/api/admin/file/upload",
        formData,
      )) as unknown as { url?: string };
      if (!uploaded.url) {
        throw new Error(t("operationFailed"));
      }
      await apiPost("/api/admin/user/myAvatar", { avatar: uploaded.url });
      return uploaded.url;
    },
    onSuccess: () => {
      setProfileMessage(t("avatarUpdated"));
      setProfileError("");
      void qc.invalidateQueries({ queryKey: ["current-user"] });
    },
    onError: (err) => {
      setProfileError((err as Error).message || t("operationFailed"));
      setProfileMessage("");
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
  const startTfaSetup = useMutation({
    mutationFn: () => apiPost<TfaSetup>("/api/admin/user/myTfaSetup"),
    onSuccess: (res) => {
      setTfaSetup(res);
      setTfaCode("");
      setSecurityMessage("");
      setSecurityError("");
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
    },
  });
  const enableTfa = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/user/myTfaEnable", {
        secret: tfaSetup?.secret ?? "",
        code: tfaCode.trim(),
      }),
    onSuccess: () => {
      setTfaSetup(null);
      setTfaCode("");
      setSecurityMessage(t("totpEnabled"));
      setSecurityError("");
      void qc.invalidateQueries({ queryKey: ["my-security"] });
      void qc.invalidateQueries({ queryKey: ["current-user"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
    },
  });
  const disableTfa = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/user/myTfaDisable", {
        code: tfaDisableCode.trim(),
      }),
    onSuccess: () => {
      setTfaDisableCode("");
      setSecurityMessage(t("totpDisabled"));
      setSecurityError("");
      void qc.invalidateQueries({ queryKey: ["my-security"] });
      void qc.invalidateQueries({ queryKey: ["current-user"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
    },
  });
  const deleteTrustedDevice = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/user/myTrustedLoginDevice/delete", { id }),
    onSuccess: () => {
      setTrustedDeleteTarget(null);
      setSecurityMessage(t("trustedDeviceDeleted"));
      setSecurityError("");
      void qc.invalidateQueries({ queryKey: ["my-security"] });
      void qc.invalidateQueries({ queryKey: ["my-trusted-login-devices"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
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

  useEffect(() => {
    let cancelled = false;
    setTfaQrDataUrl("");
    setTfaQrError("");
    if (!tfaSetup?.uri) return;

    QRCode.toDataURL(tfaSetup.uri, {
      errorCorrectionLevel: "M",
      margin: 2,
      width: 192,
    })
      .then((url) => {
        if (!cancelled) setTfaQrDataUrl(url);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setTfaQrError(
            error instanceof Error ? error.message : t("operationFailed"),
          );
        }
      });

    return () => {
      cancelled = true;
    };
  }, [t, tfaSetup?.uri]);

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
  const avatarInitial =
    (currentUser?.nickname || currentUser?.username || "")
      .trim()
      .slice(0, 1)
      .toUpperCase() || undefined;
  const securityData = security.data;
  const emailVerificationActive = Boolean(
    securityData?.effective_email_verification_enabled ??
      securityData?.email_verification_enabled,
  );
  const deviceVerificationActive = Boolean(
    securityData?.effective_login_device_verification_enabled ??
      securityData?.login_device_verification_enabled,
  );
  const emailVerificationLabel = securityData?.system_require_email_verification
    ? t("systemRequired")
    : securityData?.email_verification_enabled
      ? t("enabled")
      : t("disabled");
  const deviceVerificationLabel = securityData?.system_require_device_verification
    ? t("systemRequired")
    : securityData?.login_device_verification_enabled
      ? t("enabled")
      : t("disabled");
  const totpVerificationLabel = securityData?.tfa_enabled
    ? securityData.system_require_totp || securityData.tfa_enforced
      ? t("enforced")
      : t("enabled")
    : t("disabled");
  const copySecurityText = async (value: string) => {
    if (!value) return;
    try {
      await navigator.clipboard.writeText(value);
      setSecurityMessage(t("copied"));
      setSecurityError("");
    } catch (err) {
      setSecurityError((err as Error).message || t("operationFailed"));
    }
  };

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
      {profileMessage && (
        <InlineMessage tone="success">{profileMessage}</InlineMessage>
      )}
      {profileError && <InlineMessage tone="error">{profileError}</InlineMessage>}

      <div className="space-y-5">
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div className="flex min-w-0 items-center gap-3">
              <button
                type="button"
                className="rounded-full transition-opacity focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand disabled:cursor-not-allowed disabled:opacity-60"
                aria-label={t("uploadAvatar")}
                disabled={uploadAvatar.isPending}
                onClick={() => avatarInputRef.current?.click()}
              >
                <AvatarPreview
                  src={currentUser?.avatar}
                  alt={t("avatarPreview")}
                  fallback={avatarInitial}
                  className="size-12"
                />
              </button>
              <input
                ref={avatarInputRef}
                type="file"
                accept="image/*"
                className="sr-only"
                disabled={uploadAvatar.isPending}
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) uploadAvatar.mutate(file);
                  e.currentTarget.value = "";
                }}
              />
              <div className="min-w-0">
                <h2 className="truncate text-base font-semibold">
                  {currentUser?.nickname || currentUser?.username || t("account")}
                </h2>
                <p className="mt-1 truncate text-sm text-kumo-subtle">
                  {uploadAvatar.isPending
                    ? t("uploading")
                    : currentUser?.username || (user.isLoading ? t("loading") : "—")}
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
          <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div className="flex items-start gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <ShieldCheck size={18} />
              </div>
              <div>
                <h2 className="text-base font-semibold">{t("accountSecurity")}</h2>
                <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                  {t("accountSecurityHint")}
                </p>
              </div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                setSecurityMessage("");
                setSecurityError("");
                void qc.invalidateQueries({ queryKey: ["my-security"] });
                void qc.invalidateQueries({
                  queryKey: ["my-trusted-login-devices"],
                });
              }}
            >
              <ArrowClockwise size={16} />
              {t("refresh")}
            </Button>
          </div>

          {securityMessage && (
            <InlineMessage tone="success" className="mb-3">
              {securityMessage}
            </InlineMessage>
          )}
          {securityError && (
            <InlineMessage tone="error" className="mb-3">
              {securityError}
            </InlineMessage>
          )}
          {security.isLoading && <TableState tone="loading">{t("loading")}</TableState>}
          {security.error && (
            <TableState tone="error">
              {(security.error as Error).message || t("operationFailed")}
            </TableState>
          )}

          {security.data && (
            <div className="space-y-4">
              <div className="grid gap-3 md:grid-cols-4">
                <SecurityStateTile
                  label={t("totpVerification")}
                  value={totpVerificationLabel}
                  tone={security.data.tfa_enabled ? "success" : "default"}
                />
                <SecurityStateTile
                  label={t("emailVerification")}
                  value={emailVerificationLabel}
                  tone={emailVerificationActive ? "success" : "default"}
                />
                <SecurityStateTile
                  label={t("deviceVerification")}
                  value={deviceVerificationLabel}
                  tone={deviceVerificationActive ? "success" : "default"}
                />
                <SecurityStateTile
                  label={t("trustedLoginDevices")}
                  value={String(security.data.trusted_device_count ?? 0)}
                  tone={
                    (security.data.trusted_device_count ?? 0) > 0
                      ? "success"
                      : "default"
                  }
                />
              </div>

              <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
                <div className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                      <h3 className="text-sm font-semibold">
                        {t("totpVerification")}
                      </h3>
                      <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                        {security.data.tfa_enabled
                          ? t("totpEnabledHint")
                          : t("totpSetupHint")}
                      </p>
                    </div>
                    {!security.data.tfa_enabled && (
                      <Button
                        size="sm"
                        variant="secondary"
                        loading={startTfaSetup.isPending}
                        onClick={() => startTfaSetup.mutate()}
                      >
                        {t("setupTotp")}
                      </Button>
                    )}
                  </div>

                  {tfaSetup && (
                    <div className="mt-4 space-y-3">
                      <div className="rounded-md border border-kumo-line bg-kumo-elevated p-3">
                        <div className="grid gap-3 sm:grid-cols-[auto_minmax(0,1fr)] sm:items-start">
                          <div className="flex size-40 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-white p-2">
                            {tfaQrDataUrl ? (
                              <img
                                src={tfaQrDataUrl}
                                alt={t("totpQrCode")}
                                className="size-full"
                              />
                            ) : (
                              <span className="px-3 text-center text-xs leading-5 text-kumo-subtle">
                                {tfaQrError || t("loading")}
                              </span>
                            )}
                          </div>
                          <div className="min-w-0">
                            <div className="text-xs font-medium text-kumo-subtle">
                              {t("totpSecret")}
                            </div>
                            <code className="mt-1 block break-all font-mono text-xs">
                              {tfaSetup.secret}
                            </code>
                            <p className="mt-2 text-xs leading-5 text-kumo-subtle">
                              {t("totpQrCodeHint")}
                            </p>
                            <div className="mt-3 flex flex-wrap gap-2">
                              <Button
                                size="sm"
                                variant="secondary"
                                onClick={() => void copySecurityText(tfaSetup.secret)}
                              >
                                {t("copy")}
                              </Button>
                              <Button
                                size="sm"
                                variant="secondary"
                                onClick={() => void copySecurityText(tfaSetup.uri)}
                              >
                                {t("copyTotpUri")}
                              </Button>
                            </div>
                          </div>
                        </div>
                      </div>
                      <label className="block">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("verificationCode")}
                        </span>
                        <Input
                          aria-label={t("verificationCode")}
                          value={tfaCode}
                          inputMode="numeric"
                          autoComplete="one-time-code"
                          onChange={(e) => {
                            setTfaCode(e.target.value);
                            setSecurityError("");
                          }}
                        />
                      </label>
                      <div className="flex flex-wrap gap-2">
                        <Button
                          size="sm"
                          loading={enableTfa.isPending}
                          disabled={!tfaCode.trim()}
                          onClick={() => enableTfa.mutate()}
                        >
                          {t("enableTotp")}
                        </Button>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => {
                            setTfaSetup(null);
                            setTfaCode("");
                          }}
                        >
                          {t("cancel")}
                        </Button>
                      </div>
                    </div>
                  )}

                  {security.data.tfa_enabled && (
                    <div className="mt-4 space-y-3">
                      {security.data.tfa_enforced && (
                        <p className="rounded-md border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm leading-6 text-kumo-subtle">
                          {t("totpEnforcedSelfHint")}
                        </p>
                      )}
                      <label className="block">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("verificationCode")}
                        </span>
                        <Input
                          aria-label={t("verificationCode")}
                          value={tfaDisableCode}
                          inputMode="numeric"
                          autoComplete="one-time-code"
                          disabled={security.data.tfa_enforced}
                          onChange={(e) => {
                            setTfaDisableCode(e.target.value);
                            setSecurityError("");
                          }}
                        />
                      </label>
                      <Button
                        size="sm"
                        variant="secondary-destructive"
                        loading={disableTfa.isPending}
                        disabled={
                          security.data.tfa_enforced || !tfaDisableCode.trim()
                        }
                        onClick={() => disableTfa.mutate()}
                      >
                        {t("disableTotp")}
                      </Button>
                    </div>
                  )}
                </div>

                <div className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                  <div className="mb-3">
                    <h3 className="text-sm font-semibold">
                      {t("trustedLoginDevices")}
                    </h3>
                    <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                      {t("trustedLoginDevicesHint")}
                    </p>
                  </div>
                  <div className="rounded-lg border border-kumo-line bg-kumo-elevated">
                    <div className="divide-y divide-kumo-line">
                      {(trustedDevices.data ?? []).map((device) => (
                        <div
                          key={device.id}
                          className="flex flex-col gap-3 p-3 sm:flex-row sm:items-center sm:justify-between"
                        >
                          <div className="min-w-0">
                            <div className="break-words text-sm font-semibold">
                              {device.device_name ||
                                device.device_id ||
                                device.device_uuid ||
                                t("unknownDevice")}
                            </div>
                            <div className="mt-1 flex flex-wrap gap-2 text-xs text-kumo-subtle">
                              {device.device_os && (
                                <span className="rounded border border-kumo-line px-2 py-1">
                                  {device.device_os}
                                </span>
                              )}
                              {device.ip && (
                                <span className="rounded border border-kumo-line px-2 py-1">
                                  {device.ip}
                                </span>
                              )}
                              <span className="rounded border border-kumo-line px-2 py-1">
                                {formatUnixSeconds(device.last_seen_at)}
                              </span>
                            </div>
                          </div>
                          <Button
                            size="sm"
                            variant="secondary-destructive"
                            onClick={() => {
                              deleteTrustedDevice.reset();
                              setTrustedDeleteTarget(device);
                            }}
                          >
                            {t("delete")}
                          </Button>
                        </div>
                      ))}
                    </div>
                    {trustedDevices.isLoading && (
                      <TableState tone="loading">{t("loading")}</TableState>
                    )}
                    {trustedDevices.error && (
                      <TableState tone="error">
                        {(trustedDevices.error as Error).message ||
                          t("operationFailed")}
                      </TableState>
                    )}
                    {!trustedDevices.isLoading &&
                      !trustedDevices.error &&
                      (trustedDevices.data ?? []).length === 0 && (
                        <TableState tone="empty">
                          {t("trustedDeviceEmpty")}
                        </TableState>
                      )}
                  </div>
                </div>
              </div>
            </div>
          )}
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

      <ConfirmDialog
        open={trustedDeleteTarget !== null}
        title={t("confirmDeleteTrustedDeviceTitle")}
        description={t("confirmDeleteTrustedDeviceDescription")}
        confirmLabel={t("delete")}
        cancelLabel={t("cancel")}
        error={
          deleteTrustedDevice.error
            ? (deleteTrustedDevice.error as Error).message || t("operationFailed")
            : undefined
        }
        loading={deleteTrustedDevice.isPending}
        onOpenChange={(next) => {
          if (!next) {
            setTrustedDeleteTarget(null);
            deleteTrustedDevice.reset();
          }
        }}
        onConfirm={() => {
          if (trustedDeleteTarget) {
            deleteTrustedDevice.mutate(trustedDeleteTarget.id);
          }
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

function SecurityStateTile({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: "default" | "success";
}) {
  return (
    <div className="min-w-0 rounded-lg border border-kumo-line bg-kumo-base px-3 py-3">
      <div className="truncate text-xs font-medium text-kumo-subtle">{label}</div>
      <div className="mt-2 flex items-center gap-2">
        <span
          className={cn(
            "size-2 shrink-0 rounded-full",
            tone === "success" ? "bg-kumo-success" : "bg-kumo-fill",
          )}
          aria-hidden="true"
        />
        <span className="min-w-0 break-words text-sm font-semibold">{value}</span>
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
