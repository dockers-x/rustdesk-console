import type { ReactNode } from "react";
import { Dialog } from "@cloudflare/kumo/components/dialog";

export const dialogPanelClass =
  "overflow-hidden bg-kumo-elevated p-0 shadow-xl ring-1 ring-kumo-line";

export function DialogHeader({
  title,
  description,
}: {
  title: ReactNode;
  description?: ReactNode;
}) {
  return (
    <div className="border-b border-kumo-line px-6 py-5">
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
    <div className="max-h-[min(68vh,640px)] overflow-y-auto px-6 py-5">
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
    <div className="border-t border-kumo-line bg-kumo-recessed px-6 py-4">
      {error && (
        <p role="alert" className="mb-3 text-sm text-kumo-danger">
          {error}
        </p>
      )}
      <div className="flex flex-wrap justify-end gap-2">{children}</div>
    </div>
  );
}
