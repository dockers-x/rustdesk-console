import { WarningCircle } from "@phosphor-icons/react";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { compactDialogPanelClass } from "./DialogLayout";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  error?: string;
  loading?: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel,
  error,
  loading = false,
  onOpenChange,
  onConfirm,
}: ConfirmDialogProps) {
  return (
    <Dialog.Root role="alertdialog" open={open} onOpenChange={onOpenChange}>
      <Dialog size="sm" className={compactDialogPanelClass}>
        <div className="px-5 pt-5">
          <div className="flex items-start gap-3">
            <div className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-lg bg-kumo-danger/10 text-kumo-danger">
              <WarningCircle size={20} weight="fill" />
            </div>
            <div className="min-w-0">
              <Dialog.Title className="text-base font-semibold">
                {title}
              </Dialog.Title>
              <Dialog.Description className="mt-1 text-sm leading-6 text-kumo-subtle">
                {description}
              </Dialog.Description>
              {error && (
                <p role="alert" className="mt-3 text-sm text-kumo-danger">
                  {error}
                </p>
              )}
            </div>
          </div>
        </div>
        <div className="mt-5 flex justify-end gap-2 border-t border-kumo-line bg-kumo-recessed px-5 py-4">
          <Button
            variant="secondary"
            disabled={loading}
            onClick={() => onOpenChange(false)}
          >
            {cancelLabel}
          </Button>
          <Button variant="destructive" loading={loading} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </div>
      </Dialog>
    </Dialog.Root>
  );
}
