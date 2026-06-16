import type { ReactNode } from "react";

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
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={`m-3 rounded-md border px-3 py-2 text-sm ${TONE_CLASS[tone]}`}
    >
      {children}
    </div>
  );
}
