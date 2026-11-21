import type { ReactNode } from "react";
import { CircleNotch, Tray, WarningCircle } from "@phosphor-icons/react";

type TableStateTone = "loading" | "empty" | "error";

const TONE_CLASS: Record<TableStateTone, string> = {
  loading: "text-kumo-subtle",
  empty: "text-kumo-subtle",
  error: "text-kumo-danger",
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
      className={`flex min-h-28 flex-col items-center justify-center gap-2.5 px-4 py-5 text-center text-sm ${TONE_CLASS[tone]}`}
    >
      <span aria-hidden className={`grid size-9 place-items-center rounded-xl ${tone === "error" ? "bg-kumo-danger/10" : "bg-kumo-tint"}`}>
        <Icon size={20} className={tone === "loading" ? "animate-spin motion-reduce:animate-none" : undefined} />
      </span>
      {children}
    </div>
  );
}
