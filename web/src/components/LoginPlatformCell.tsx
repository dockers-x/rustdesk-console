import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Info } from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { summarizeLoginPlatform } from "../lib/loginPlatform";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "./DialogLayout";

export function LoginPlatformCell({ value }: { value: string }) {
  const { t } = useTranslation();
  const { summary, raw } = summarizeLoginPlatform(value);
  if (!raw) return "—";
  if (summary.toLowerCase() === raw.toLowerCase() && raw.length <= 38) return summary;

  return (
    <Dialog.Root>
      <Dialog.Trigger
        render={
          <Button
            size="sm"
            variant="ghost"
            icon={<Info aria-hidden="true" size={16} />}
            className="-ml-2 max-w-52 justify-start px-2"
            aria-label={summary ? `${summary} · ${t("loginPlatformDetails")}` : t("loginPlatformDetails")}
          />
        }
      >
        <span className="truncate">{summary || t("loginPlatformDetails")}</span>
      </Dialog.Trigger>
      <Dialog className={dialogPanelClass}>
        <DialogHeader title={t("loginPlatformDetails")} />
        <DialogBody>
          <pre className="select-text whitespace-pre-wrap break-all rounded-md border border-kumo-line bg-kumo-recessed p-3 text-xs leading-relaxed">
            {raw}
          </pre>
        </DialogBody>
        <DialogFooter><Dialog.Close render={<Button variant="secondary" />}>{t("close")}</Dialog.Close></DialogFooter>
      </Dialog>
    </Dialog.Root>
  );
}
