import type { ReactNode } from "react";
import { Dialog } from "@cloudflare/kumo/components/dialog";

const dialogBaseClass =
  "admin-dialog-panel z-50 border border-kumo-line p-0 text-kumo-default shadow-lg ring-0";

export const dialogPanelClass =
  `${dialogBaseClass} admin-dialog-panel--default bg-kumo-elevated`;

export const compactDialogPanelClass =
  `${dialogBaseClass} admin-dialog-panel--compact bg-kumo-elevated`;

export const resourceFormDialogPanelClass =
  `${dialogBaseClass} admin-dialog-panel--wide bg-kumo-base`;

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
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-5 [scrollbar-gutter:stable] sm:px-6">
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
