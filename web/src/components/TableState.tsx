import type { ReactNode } from "react";
import { CircleNotch, Tray, WarningCircle } from "@phosphor-icons/react";

type TableStateTone = "loading" | "empty" | "error";

const TONE_CLASS: Record<TableStateTone, string> = {
  loading: "border-kumo-line bg-kumo-elevated text-kumo-subtle",
  empty: "border-kumo-line bg-kumo-elevated text-kumo-subtle",
  error: "border-kumo-danger/25 bg-kumo-danger/10 text-kumo-danger",
};

export function TableState({
  tone,
  children,
}: {
  tone: TableStateTone;
  children: ReactNode;
}) {
  const Icon = tone === "loading" ? CircleNotch : tone === "error" ? WarningCircle : Tray;
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={`m-3 flex min-h-20 flex-col items-center justify-center gap-2 rounded-lg border border-dashed px-4 py-5 text-center text-sm ${TONE_CLASS[tone]}`}
    >
      <Icon size={22} aria-hidden="true" className={tone === "loading" ? "animate-spin motion-reduce:animate-none" : undefined} />
      {children}
    </div>
  );
}
