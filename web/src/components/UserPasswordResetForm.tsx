import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { apiPost } from "../lib/api";
import { DialogBody, DialogFooter } from "./DialogLayout";
import { InlineMessage } from "./InlineMessage";

export function UserPasswordResetForm({ id, email, onClose }: {
  id: number;
  email: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const emailReset = useMutation({
    mutationFn: () => apiPost("/api/admin/user/password-reset/email", { id }),
  });
  const manualReset = useMutation({
    mutationFn: () => apiPost("/api/admin/user/changePwd", {
      id,
      password,
      confirmPassword: confirm,
    }),
    onSuccess: () => {
      emailReset.reset();
      setPassword("");
      setConfirm("");
    },
  });
  const pending = emailReset.isPending || manualReset.isPending;
  const mismatch = confirm.length > 0 && password !== confirm;
  const valid = password.length >= 4 && password === confirm;

  return (
    <>
      <DialogBody>
        <div className="grid gap-5">
          <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-4">
            <h3 className="font-semibold">{t("userResetByEmail")}</h3>
            <p className="mt-1 break-words text-sm text-kumo-subtle">
              {email ? t("userResetMailRecipient", { email }) : t("userResetNoEmail")}
            </p>
            {email && (
              <Button variant="secondary" className="mt-3"
                disabled={pending || emailReset.isSuccess} loading={emailReset.isPending}
                onClick={() => emailReset.mutate()}>
                {t("userSendResetMail")}
              </Button>
            )}
            {emailReset.isSuccess && <InlineMessage tone="success" className="mt-3">{t("userResetMailSent")}</InlineMessage>}
            {emailReset.error && <InlineMessage tone="error" className="mt-3">{emailReset.error.message}</InlineMessage>}
          </section>

          <form className="grid gap-3 rounded-lg border border-kumo-line bg-kumo-elevated p-4"
            onSubmit={(event) => { event.preventDefault(); if (valid && !pending) manualReset.mutate(); }}>
            <div>
              <h3 className="font-semibold">{t("userResetManually")}</h3>
              <p className="mt-1 text-sm text-kumo-subtle">{t("userResetRevokesSessions")}</p>
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <Input type="password" label={t("newPassword")} autoComplete="new-password"
                minLength={4} required value={password} onChange={(event) => {
                  manualReset.reset();
                  setPassword(event.target.value);
                }} />
              <Input type="password" label={t("confirmPassword")} autoComplete="new-password"
                required aria-invalid={mismatch} aria-describedby={mismatch ? "admin-password-mismatch" : undefined}
                value={confirm} onChange={(event) => {
                  manualReset.reset();
                  setConfirm(event.target.value);
                }} />
            </div>
            {mismatch && <p id="admin-password-mismatch" className="text-sm text-kumo-danger">{t("passwordMismatch")}</p>}
            {manualReset.isSuccess && <InlineMessage tone="success">{t("userResetManualDone")}</InlineMessage>}
            {manualReset.error && <InlineMessage tone="error">{manualReset.error.message}</InlineMessage>}
            <div className="flex">
              <Button type="submit" loading={manualReset.isPending} disabled={!valid || pending}>{t("userSetPassword")}</Button>
            </div>
          </form>
        </div>
      </DialogBody>
      <DialogFooter><Button variant="secondary" disabled={pending} onClick={onClose}>{t("close")}</Button></DialogFooter>
    </>
  );
}
