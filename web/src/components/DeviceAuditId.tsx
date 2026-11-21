import { Button } from "@cloudflare/kumo/components/button";
import { Link } from "@cloudflare/kumo/components/link";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { CopySimple } from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { Link as RouterLink } from "react-router-dom";
import { copyToClipboard } from "../lib/clipboard";

type DeviceTarget = "address_book" | "device";

function readDevice(value: unknown) {
  if (!value || typeof value !== "object") return undefined;
  const { target, name, note } = value as { target?: unknown; name?: unknown; note?: unknown };
  if (target !== "address_book" && target !== "device") return undefined;
  return {
    target,
    name: typeof name === "string" ? name.trim() : "",
    note: typeof note === "string" ? note.trim() : "",
  };
}

export function DeviceAuditId({
  id: value,
  device,
  fallbackTarget,
}: {
  id: unknown;
  device?: unknown;
  fallbackTarget?: DeviceTarget;
}) {
  const { t } = useTranslation();
  const toasts = useKumoToastManager();
  const id = typeof value === "string" ? value : "";
  if (!id.trim()) return "—";

  const reference = readDevice(device);
  const target = reference?.target ?? fallbackTarget;
  const primary = reference?.name || reference?.note || id;
  const copyLabel = `${t("copy")} ${t("deviceId")} ${id}`;
  const idClass = "block max-w-40 truncate font-mono text-sm tabular-nums";
  const copy = async () => {
    if (await copyToClipboard(id)) {
      toasts.add({ title: t("copied"), variant: "success" });
    } else {
      toasts.add({ title: t("operationFailed"), variant: "error" });
    }
  };

  return (
    <span className="audit-device-id inline-flex items-center gap-1">
      <span className="min-w-0">
        {target ? (
          <Link
            render={
              <RouterLink to={{
                pathname: target === "address_book" ? "/address_book" : "/peers",
                search: new URLSearchParams({ id }).toString(),
              }} />
            }
            className={primary === id ? idClass : "block max-w-40 truncate text-sm font-medium"}
            title={[primary, reference?.note, t(target === "address_book" ? "addressBook" : "openDeviceInList")]
              .filter((text, index, items) => text && items.indexOf(text) === index)
              .join("\n")}
          >
            {primary}
          </Link>
        ) : <span className={idClass} title={id}>{id}</span>}
        {primary !== id && (
          <span className={`${idClass} text-kumo-subtle`} title={id}>{id}</span>
        )}
      </span>
      <Button
        type="button"
        size="sm"
        shape="square"
        variant="ghost"
        className="audit-device-copy size-9 shrink-0 text-kumo-subtle hover:text-kumo-default max-sm:size-11"
        aria-label={copyLabel}
        title={copyLabel}
        onClick={() => void copy()}
        icon={<CopySimple size={16} aria-hidden />}
      />
    </span>
  );
}
