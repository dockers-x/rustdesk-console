import type { ReactNode } from "react";
import { CheckCircle, WarningCircle } from "@phosphor-icons/react";

type InlineMessageTone = "error" | "success";

const TONE_CLASS: Record<InlineMessageTone, string> = {
  error: "border-kumo-danger/25 bg-kumo-danger-tint/40 text-kumo-danger",
  success: "border-kumo-success/25 bg-kumo-success-tint/60 text-kumo-success",
};

export function InlineMessage({
  tone,
  children,
  className = "",
}: {
  tone: InlineMessageTone;
  children: ReactNode;
  className?: string;
}) {
  const Icon = tone === "error" ? WarningCircle : CheckCircle;
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={`flex items-start gap-2 rounded-lg border px-3 py-2.5 text-sm leading-5 ${TONE_CLASS[tone]} ${className}`}
    >
      <Icon size={18} aria-hidden className="mt-px shrink-0" />
      <div className="min-w-0 [overflow-wrap:anywhere]">{children}</div>
    </div>
  );
}
