import type { ReactNode } from "react";
import { Dialog } from "@cloudflare/kumo/components/dialog";

export const dialogPanelClass =
  "overflow-hidden bg-kumo-elevated p-0 ring-1 ring-kumo-line";

export const resourceFormDialogPanelClass =
  "z-50 flex max-h-[calc(100svh-1rem)] min-w-0 w-[calc(100vw-1rem)] max-w-[calc(100vw-1rem)] flex-col overflow-hidden border border-kumo-line bg-kumo-base p-0 text-kumo-default ring-2 ring-kumo-line drop-shadow-xl drop-shadow-kumo-shadow-drop sm:max-h-[min(86vh,720px)] sm:w-[min(42rem,calc(100vw-2rem))] sm:max-w-[min(42rem,calc(100vw-2rem))]";

export function DialogHeader({
  title,
  description,
}: {
  title: ReactNode;
  description?: ReactNode;
}) {
  return (
    <div className="shrink-0 border-b border-kumo-line px-4 py-4 sm:px-6 sm:py-5">
      <Dialog.Title className="text-lg font-semibold">{title}</Dialog.Title>
      {description && (
        <Dialog.Description className="mt-1 text-sm leading-6 text-kumo-subtle">
          {description}
        </Dialog.Description>
      )}
    </div>
  );
}

export function DialogBody({ children }: { children: ReactNode }) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-5 sm:px-6">
      {children}
    </div>
  );
}

export function DialogFooter({
  children,
  error,
}: {
  children: ReactNode;
  error?: ReactNode;
}) {
  return (
    <div className="shrink-0 border-t border-kumo-line bg-kumo-recessed px-4 py-4 sm:px-6">
      {error && (
        <p role="alert" className="mb-3 text-sm text-kumo-danger">
          {error}
        </p>
      )}
      <div className="flex flex-wrap justify-end gap-2">{children}</div>
    </div>
  );
}
