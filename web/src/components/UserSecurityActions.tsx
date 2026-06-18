import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Switch } from "@cloudflare/kumo/components/switch";
import { ShieldCheck } from "@phosphor-icons/react";
import { cn } from "@cloudflare/kumo/utils";
import { ConfirmDialog } from "./ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "./DialogLayout";
import { InlineMessage } from "./InlineMessage";
import { TableState } from "./TableState";
import { apiGet, apiPost, ApiError } from "../lib/api";
import { formatUnixSeconds } from "../lib/dateFormat";

interface UserSecurityRow {
  id?: unknown;
  username?: unknown;
  email?: unknown;
  tfa_enabled?: unknown;
  tfa_enforced?: unknown;
  email_verification_enabled?: unknown;
  login_device_verification_enabled?: unknown;
  trusted_device_count?: unknown;
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

interface SecurityForm {
  tfa_enforced: boolean;
  email_verification_enabled: boolean;
  login_device_verification_enabled: boolean;
}

function boolValue(value: unknown) {
  return value === true || value === 1 || value === "true" || value === "1";
}

function numberValue(value: unknown) {
  const n = Number(value);
  return Number.isFinite(n) ? n : 0;
}

export function UserSecurityActions({ row }: { row: UserSecurityRow }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [resetOpen, setResetOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<TrustedLoginDevice | null>(null);
  const userId = numberValue(row.id);
  const username = String(row.username || row.email || row.id || "");
  const tfaEnabled = boolValue(row.tfa_enabled);
  const [form, setForm] = useState<SecurityForm>({
    tfa_enforced: boolValue(row.tfa_enforced),
    email_verification_enabled: boolValue(row.email_verification_enabled),
    login_device_verification_enabled: boolValue(
      row.login_device_verification_enabled,
    ),
  });

  useEffect(() => {
    setForm({
      tfa_enforced: boolValue(row.tfa_enforced),
      email_verification_enabled: boolValue(row.email_verification_enabled),
      login_device_verification_enabled: boolValue(
        row.login_device_verification_enabled,
      ),
    });
  }, [
    row.tfa_enforced,
    row.email_verification_enabled,
    row.login_device_verification_enabled,
  ]);

  const devices = useQuery({
    queryKey: ["user-trusted-login-devices", userId],
    enabled: open && userId > 0,
    queryFn: () =>
      apiGet<TrustedLoginDevice[]>("/api/admin/user/trusted-login-devices", {
        user_id: userId,
      }),
  });

  const save = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/user/security", {
        id: userId,
        ...form,
      }),
    onSuccess: () => {
      setMessage(t("userSecuritySaved"));
      setError("");
      void qc.invalidateQueries({ queryKey: ["users"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
      setMessage("");
    },
  });

  const resetTfa = useMutation({
    mutationFn: () => apiPost("/api/admin/user/tfa/reset", { id: userId }),
    onSuccess: () => {
      setResetOpen(false);
      setMessage(t("userTotpReset"));
      setError("");
      void qc.invalidateQueries({ queryKey: ["users"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    },
  });

  const deleteDevice = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/user/trusted-login-device/delete", { id }),
    onSuccess: () => {
      setDeleteTarget(null);
      setMessage(t("trustedDeviceDeleted"));
      setError("");
      void devices.refetch();
      void qc.invalidateQueries({ queryKey: ["users"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    },
  });

  if (!userId) return null;

  return (
    <>
      <Button
        size="sm"
        variant="secondary"
        onClick={() => {
          setOpen(true);
          setMessage("");
          setError("");
        }}
      >
        <ShieldCheck size={14} />
        {t("security")}
      </Button>

      <Dialog.Root open={open} onOpenChange={setOpen}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader
            title={`${t("accountSecurity")} · ${username || userId}`}
            description={t("userSecurityManagementHint")}
          />
          <DialogBody>
            <div className="space-y-4">
              {message && <InlineMessage tone="success">{message}</InlineMessage>}
              {error && <InlineMessage tone="error">{error}</InlineMessage>}

              <div className="grid gap-3 sm:grid-cols-3">
                <SecuritySummary
                  label={t("totpVerification")}
                  value={
                    tfaEnabled
                      ? form.tfa_enforced
                        ? t("enforced")
                        : t("enabled")
                      : form.tfa_enforced
                        ? t("pendingSetup")
                        : t("disabled")
                  }
                  active={tfaEnabled}
                />
                <SecuritySummary
                  label={t("emailVerification")}
                  value={
                    form.email_verification_enabled ? t("enabled") : t("disabled")
                  }
                  active={form.email_verification_enabled}
                />
                <SecuritySummary
                  label={t("trustedLoginDevices")}
                  value={String(row.trusted_device_count ?? 0)}
                  active={numberValue(row.trusted_device_count) > 0}
                />
              </div>

              <div className="grid gap-3">
                <AdminSecuritySwitch
                  label={t("forceUserTotp")}
                  description={
                    tfaEnabled
                      ? t("forceUserTotpHint")
                      : t("forceUserTotpPendingHint")
                  }
                  checked={form.tfa_enforced}
                  onCheckedChange={(value) =>
                    setForm((current) => ({ ...current, tfa_enforced: value }))
                  }
                />
                <AdminSecuritySwitch
                  label={t("userEmailVerification")}
                  description={t("userEmailVerificationHint")}
                  checked={form.email_verification_enabled}
                  onCheckedChange={(value) =>
                    setForm((current) => ({
                      ...current,
                      email_verification_enabled: value,
                    }))
                  }
                />
                <AdminSecuritySwitch
                  label={t("userDeviceVerification")}
                  description={t("userDeviceVerificationHint")}
                  checked={form.login_device_verification_enabled}
                  onCheckedChange={(value) =>
                    setForm((current) => ({
                      ...current,
                      login_device_verification_enabled: value,
                    }))
                  }
                />
              </div>

              <div className="rounded-lg border border-kumo-line bg-kumo-base">
                <div className="flex flex-col gap-3 border-b border-kumo-line p-3 sm:flex-row sm:items-center sm:justify-between">
                  <div>
                    <h3 className="text-sm font-semibold">
                      {t("trustedLoginDevices")}
                    </h3>
                    <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                      {t("adminTrustedDevicesHint")}
                    </p>
                  </div>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => void devices.refetch()}
                  >
                    {t("refresh")}
                  </Button>
                </div>
                <div className="divide-y divide-kumo-line">
                  {(devices.data ?? []).map((device) => (
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
                          deleteDevice.reset();
                          setDeleteTarget(device);
                        }}
                      >
                        {t("delete")}
                      </Button>
                    </div>
                  ))}
                </div>
                {devices.isLoading && (
                  <TableState tone="loading">{t("loading")}</TableState>
                )}
                {devices.error && (
                  <TableState tone="error">
                    {(devices.error as Error).message || t("operationFailed")}
                  </TableState>
                )}
                {!devices.isLoading &&
                  !devices.error &&
                  (devices.data ?? []).length === 0 && (
                    <TableState tone="empty">{t("trustedDeviceEmpty")}</TableState>
                  )}
              </div>
            </div>
          </DialogBody>
          <DialogFooter
            error={
              save.error
                ? (save.error as Error).message || t("operationFailed")
                : undefined
            }
          >
            {tfaEnabled && (
              <Button
                variant="secondary-destructive"
                disabled={resetTfa.isPending}
                onClick={() => {
                  resetTfa.reset();
                  setResetOpen(true);
                }}
              >
                {t("resetTotp")}
              </Button>
            )}
            <div className="flex-1" />
            <Button variant="secondary" onClick={() => setOpen(false)}>
              {t("close")}
            </Button>
            <Button
              loading={save.isPending}
              disabled={save.isPending}
              onClick={() => save.mutate()}
            >
              {t("save")}
            </Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>

      <ConfirmDialog
        open={resetOpen}
        title={t("confirmResetTotpTitle")}
        description={t("confirmResetTotpDescription")}
        confirmLabel={t("resetTotp")}
        cancelLabel={t("cancel")}
        error={
          resetTfa.error
            ? (resetTfa.error as Error).message || t("operationFailed")
            : undefined
        }
        loading={resetTfa.isPending}
        onOpenChange={(next) => {
          if (!next) {
            setResetOpen(false);
            resetTfa.reset();
          }
        }}
        onConfirm={() => resetTfa.mutate()}
      />

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("confirmDeleteTrustedDeviceTitle")}
        description={t("confirmDeleteTrustedDeviceDescription")}
        confirmLabel={t("delete")}
        cancelLabel={t("cancel")}
        error={
          deleteDevice.error
            ? (deleteDevice.error as Error).message || t("operationFailed")
            : undefined
        }
        loading={deleteDevice.isPending}
        onOpenChange={(next) => {
          if (!next) {
            setDeleteTarget(null);
            deleteDevice.reset();
          }
        }}
        onConfirm={() => {
          if (deleteTarget) deleteDevice.mutate(deleteTarget.id);
        }}
      />
    </>
  );
}

function SecuritySummary({
  label,
  value,
  active,
}: {
  label: string;
  value: string;
  active: boolean;
}) {
  return (
    <div className="min-w-0 rounded-lg border border-kumo-line bg-kumo-base px-3 py-3">
      <div className="truncate text-xs font-medium text-kumo-subtle">{label}</div>
      <div className="mt-2 flex items-center gap-2">
        <span
          className={cn(
            "size-2 shrink-0 rounded-full",
            active ? "bg-kumo-success" : "bg-kumo-fill",
          )}
          aria-hidden="true"
        />
        <span className="min-w-0 break-words text-sm font-semibold">{value}</span>
      </div>
    </div>
  );
}

function AdminSecuritySwitch({
  label,
  description,
  checked,
  onCheckedChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}) {
  return (
    <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-3">
      <Switch
        label={label}
        controlFirst={false}
        checked={checked}
        onCheckedChange={onCheckedChange}
      />
      <p className="mt-1 text-sm leading-6 text-kumo-subtle">{description}</p>
    </div>
  );
}
