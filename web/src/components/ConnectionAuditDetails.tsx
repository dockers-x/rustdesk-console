import { type ReactNode } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { useTranslation } from "react-i18next";
import { usePublicAdminConfig } from "../lib/adminTitle";
import { formatDateTime, formatUnixSeconds } from "../lib/dateFormat";
import { DeviceAuditId } from "./DeviceAuditId";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "./DialogLayout";

type AuditRow = Record<string, unknown>;

const durationUnits = [
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
  ["second", 1],
] as const;

function formatDuration(seconds: number, language: string) {
  return durationUnits.flatMap(([unit, scale]) => {
    const amount = Math.floor(seconds / scale);
    seconds %= scale;
    if (amount === 0 && unit !== "second") return [];
    return [new Intl.NumberFormat(language, { style: "unit", unit, unitDisplay: "short" }).format(amount)];
  }).join(" ");
}

export function ConnectionAuditDuration({ row }: { row: AuditRow }) {
  const { t, i18n } = useTranslation();
  if (row.end_status === "active") return <span className="text-kumo-subtle">{t("auditReportedActive")}</span>;
  if (row.end_status !== "closed") return <span className="text-kumo-subtle">{t("auditEndUnknown")}</span>;
  if (typeof row.duration_seconds !== "number") return "—";
  return <span className="whitespace-nowrap tabular-nums">{formatDuration(row.duration_seconds, i18n.language)}</span>;
}

export function ConnectionAuditDevice({ row }: { row: AuditRow }) {
  const { data } = usePublicAdminConfig();
  const started = formatDateTime(row.created_at, data?.audit_timezone ?? "UTC");
  return (
    <>
      <DeviceAuditId id={row.peer_id} device={row.peer_id_device} />
      <div className="grid max-w-44 gap-1 pb-1 sm:hidden">
        <span className="whitespace-nowrap tabular-nums">{started || "—"}</span>
        <ConnectionAuditDuration row={row} />
        {typeof row.note === "string" && row.note.trim() && (
          <span className="truncate text-kumo-subtle" title={row.note}>{row.note}</span>
        )}
      </div>
    </>
  );
}

export function ConnectionAuditDetails({ row }: { row: AuditRow }) {
  const { t } = useTranslation();
  const { data } = usePublicAdminConfig();
  const zone = data?.audit_timezone ?? "UTC";
  const closeTime = typeof row.close_time === "number" && row.close_time > 0 && row.close_time <= 8.64e12
    ? formatUnixSeconds(row.close_time, zone)
    : undefined;
  const endTime = closeTime || t(row.end_status === "active" ? "auditReportedActive" : "auditEndUnknown");
  const reportedFields = [
    ["id", "id"],
    ["action", "action"],
    ["conn_id", "connId"],
    ["session_id", "auditSessionId"],
    ["type", "type"],
    ["uuid", "uuid"],
    ["from_name", "fromName"],
    ["ip", "ip"],
  ] as const;

  return (
    <Dialog.Root>
      <Dialog.Trigger render={<Button size="sm" variant="ghost" className="min-h-11 sm:min-h-0" />}>
        {t("auditViewDetails")}
      </Dialog.Trigger>
      <Dialog className={dialogPanelClass}>
        <DialogHeader title={t("auditConnectionDetails")} />
        <DialogBody>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
            <Detail title={t("deviceId")}><DeviceAuditId id={row.peer_id} device={row.peer_id_device} /></Detail>
            <Detail title={t("fromPeer")}><DeviceAuditId id={row.from_peer} device={row.from_peer_device} /></Detail>
            <Detail title={t("auditStartedAt")}>{formatDateTime(row.created_at, zone) || "—"}</Detail>
            <Detail title={t("auditClosedAt")}>{endTime}</Detail>
            <Detail title={t("auditDuration")}><ConnectionAuditDuration row={row} /></Detail>
            <Detail title={t("updatedAt")}>{formatDateTime(row.updated_at, zone) || "—"}</Detail>
            <div className="col-span-2 border-y border-kumo-line py-3">
              <dt className="mb-0.5 text-kumo-subtle">{t("auditReportedNote")}</dt>
              <dd className="wrap-anywhere whitespace-pre-wrap">
                {typeof row.note === "string" && row.note ? row.note : "—"}
              </dd>
            </div>
            {reportedFields.map(([field, label]) => (
              <Detail key={field} title={t(label)}>
                {row[field] == null || row[field] === "" ? "—" : String(row[field])}
              </Detail>
            ))}
          </dl>
        </DialogBody>
        <DialogFooter>
          <Dialog.Close render={<Button variant="secondary" />}>{t("close")}</Dialog.Close>
        </DialogFooter>
      </Dialog>
    </Dialog.Root>
  );
}

function Detail({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="mb-0.5 text-kumo-subtle">{title}</dt>
      <dd className="wrap-anywhere">{children}</dd>
    </div>
  );
}
