import type { ReactNode } from "react";

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
  return (
    <p
      role={tone === "error" ? "alert" : "status"}
      className={`rounded-md border px-3 py-2 text-sm ${TONE_CLASS[tone]} ${className}`}
    >
      {children}
    </p>
  );
}
