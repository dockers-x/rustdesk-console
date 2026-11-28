import { useId, useState, type FormEvent } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { ApiError, apiPost } from "../lib/api";
import { compactDialogPanelClass, DialogBody, DialogFooter, DialogHeader } from "./DialogLayout";
import { InlineMessage } from "./InlineMessage";

export function DisableTotpDialog({
  enforced,
  onDisabled,
}: {
  enforced: boolean;
  onDisabled: () => void;
}) {
  const { t } = useTranslation();
  const errorId = useId();
  const [open, setOpen] = useState(false);
  const [code, setCode] = useState("");
  const disable = useMutation({
    mutationFn: () => apiPost("/api/admin/user/myTfaDisable", { code }),
    gcTime: 0,
    onSuccess: () => {
      setCode("");
      setOpen(false);
      onDisabled();
    },
  });
  const error = disable.error instanceof ApiError
    ? disable.error.message || t("operationFailed")
    : disable.isError
      ? t("operationFailed")
      : undefined;

  function changeOpen(next: boolean) {
    if (disable.isPending) return;
    setOpen(next);
    if (!next) {
      setCode("");
      disable.reset();
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (code.length === 6 && !enforced && !disable.isPending) {
      disable.mutate();
    }
  }

  return (
    <Dialog.Root open={open} onOpenChange={changeOpen}>
      <Dialog.Trigger render={
        <Button size="sm" variant="secondary-destructive" className="max-sm:min-h-11" disabled={enforced} />
      }>
        {t("disableTotp")}
      </Dialog.Trigger>
      <Dialog size="sm" className={compactDialogPanelClass}>
        <DialogHeader title={t("disableTotp")} description={t("totpDisableHint")} />
        <form onSubmit={submit}>
          <DialogBody>
            <label className="block">
              <span className="mb-1.5 block text-sm font-medium">
                {t("verificationCode")}
              </span>
              <Input
                className="max-sm:min-h-11"
                value={code}
                inputMode="numeric"
                autoComplete="one-time-code"
                aria-describedby={error ? errorId : undefined}
                readOnly={disable.isPending}
                onChange={(event) => {
                  setCode(event.currentTarget.value.replace(/\D/g, "").slice(0, 6));
                  if (disable.isError) disable.reset();
                }}
              />
            </label>
            {error && (
              <div id={errorId} className="mt-3">
                <InlineMessage tone="error">{error}</InlineMessage>
              </div>
            )}
          </DialogBody>
          <DialogFooter>
            <Dialog.Close render={
              <Button type="button" variant="secondary" className="max-sm:min-h-11" disabled={disable.isPending} />
            }>
              {t("cancel")}
            </Dialog.Close>
            <Button
              type="submit"
              variant="destructive"
              className="max-sm:min-h-11"
              disabled={code.length !== 6 || enforced || disable.isPending}
              loading={disable.isPending}
            >
              {t("disableTotp")}
            </Button>
          </DialogFooter>
        </form>
      </Dialog>
    </Dialog.Root>
  );
}
