import { type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { ArrowDownLeftIcon, ArrowUpRightIcon } from "@phosphor-icons/react";
import { DeviceAuditId } from "./DeviceAuditId";
import { usePublicAdminConfig } from "../lib/adminTitle";
import { formatDateTime } from "../lib/dateFormat";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "./DialogLayout";

type AuditRow = Record<string, unknown>;

function text(value: unknown) {
  return typeof value === "string" && value.length ? value : undefined;
}

function fileEntry(value: unknown): { name: string; size_bytes?: string }[] {
  if (!value || typeof value !== "object" || !("name" in value) || typeof value.name !== "string") {
    return [];
  }
  return [{
    name: value.name,
    size_bytes: "size_bytes" in value ? text(value.size_bytes) : undefined,
  }];
}

export function FileAuditDirection({ row }: { row: AuditRow }) {
  const { t } = useTranslation();
  const direction = row.direction;
  if (direction !== "sent" && direction !== "received") return <Missing />;
  const DirectionIcon = direction === "sent" ? ArrowUpRightIcon : ArrowDownLeftIcon;
  return (
    <span className="inline-flex max-w-36 items-start gap-1.5">
      <DirectionIcon aria-hidden size={16} className="h-lh shrink-0 text-kumo-subtle" />
      {t(direction === "sent" ? "fileAuditSent" : "fileAuditReceived")}
    </span>
  );
}

function Missing() {
  const { t } = useTranslation();
  return <span className="text-kumo-subtle">{t("fileAuditNotReported")}</span>;
}

export function FileAuditPeer({ row, remote = false }: { row: AuditRow; remote?: boolean }) {
  const localId = text(row.peer_id);
  // Older sparse reports could store their reporting ID in both fields.
  const remoteId = row.conn_id == null && row.from_peer === localId ? undefined : text(row.from_peer);
  const id = remote ? remoteId : localId;
  const name = remote && text(row.from_name);
  if (!id && !name) return <Missing />;
  return (
    <div className="max-w-44 space-y-0.5 break-all">
      {id && <DeviceAuditId id={id} device={remote ? row.from_peer_device : row.peer_id_device} />}
      {name && <span className="block text-kumo-subtle">{name}</span>}
    </div>
  );
}

export function FileAuditPath({ row }: { row: AuditRow }) {
  const files = Array.isArray(row.files) ? row.files.flatMap(fileEntry) : [];
  const path = text(row.path) ?? files.find((file) => file.name)?.name;
  return path ? (
    <span className="block max-w-44 truncate font-mono" title={path}>{path}</span>
  ) : <Missing />;
}

function FileSize({ bytes }: { bytes: unknown }) {
  const { i18n } = useTranslation();
  const value = text(bytes);
  if (!value || !/^\d+$/.test(value)) return <Missing />;
  const exact = BigInt(value);
  const units = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte", "petabyte"] as const;
  const scale = exact === 0n ? 0 : Math.min(units.length - 1, Math.floor((value.length - 1) / 3));
  const format = (unit: typeof units[number]) =>
    new Intl.NumberFormat(i18n.language, { style: "unit", unit, maximumFractionDigits: 1 });
  return (
    <span title={scale ? format("byte").format(exact) : undefined}>
      {scale ? format(units[scale]).format(Number(exact) / 1000 ** scale) : format("byte").format(exact)}
    </span>
  );
}

export function FileAuditTotals({ row }: { row: AuditRow }) {
  const { t, i18n } = useTranslation();
  const count = typeof row.file_count === "number" ? row.file_count : undefined;
  const hasSize = typeof row.size_bytes === "string";
  if (count === undefined && !hasSize) return <Missing />;
  return (
    <div className="grid gap-0.5 whitespace-nowrap tabular-nums">
      {count !== undefined && (
        <span>{t("fileAuditCount", { count, number: new Intl.NumberFormat(i18n.language).format(count) })}</span>
      )}
      <span className="text-kumo-subtle">
        {hasSize ? <>
          <FileSize bytes={row.size_bytes} />{row.size_is_partial === true && ` · ${t("fileAuditListedSize")}`}
        </> : <Missing />}
      </span>
    </div>
  );
}

export function FileAuditSummary({ row }: { row: AuditRow }) {
  const adminConfig = usePublicAdminConfig();
  const timestamp = formatDateTime(row.created_at, adminConfig.data?.timezone?.trim() || undefined);
  return (
    <div className="grid max-w-44 gap-1">
      <span className="whitespace-nowrap tabular-nums">{timestamp || "—"}</span>
      <div className="grid gap-1 sm:hidden">
        <FileAuditPeer row={row} />
        <FileAuditDirection row={row} />
        <FileAuditPath row={row} />
        <FileAuditTotals row={row} />
      </div>
    </div>
  );
}

export function FileAuditDetails({ row }: { row: AuditRow }) {
  const { t } = useTranslation();
  const adminConfig = usePublicAdminConfig();
  const timestamp = formatDateTime(row.created_at, adminConfig.data?.timezone?.trim() || undefined);
  const files = Array.isArray(row.files) ? row.files.flatMap(fileEntry) : [];
  const path = text(row.path);
  const ip = text(row.ip);
  return (
    <Dialog.Root>
      <Dialog.Trigger render={<Button size="sm" variant="ghost" className="min-h-11 sm:min-h-0" />}>
        {t("fileAuditOpenDetails")}
      </Dialog.Trigger>
      <Dialog className={dialogPanelClass}>
        <DialogHeader title={t("fileAuditDetails")} description={timestamp || undefined} />
        <DialogBody>
          <div className="space-y-5 text-sm">
            <dl className="grid grid-cols-1 gap-x-4 gap-y-3 sm:grid-cols-2">
              <Detail title={t("fileAuditReportingDevice")}><FileAuditPeer row={row} /></Detail>
              <Detail title={t("fileAuditRemoteDevice")}><FileAuditPeer row={row} remote /></Detail>
              <Detail title={t("fileAuditDirection")}><FileAuditDirection row={row} /></Detail>
              <Detail title={t("fileAuditFilesAndSize")}><FileAuditTotals row={row} /></Detail>
              {ip && <Detail title={t("fileAuditRemoteIp")}><span className="font-mono">{ip}</span></Detail>}
              {typeof row.conn_id === "number" && <Detail title={t("fileAuditConnectionId")}>{row.conn_id}</Detail>}
              {path && (
                <Detail title={t("fileAuditReportedPath")} className="sm:col-span-2">
                  <span className="wrap-anywhere font-mono">{path}</span>
                </Detail>
              )}
            </dl>
            <section className="space-y-2 border-t border-kumo-line pt-4">
              <h3 className="font-semibold">{t("fileAuditReportedFiles")}</h3>
              {row.size_is_partial === true && files.length > 0 && (
                <p className="text-kumo-subtle">{t("fileAuditSampleHint")}</p>
              )}
              {files.length > 0 ? (
                <ul className="divide-y divide-kumo-line rounded-lg border border-kumo-line">
                  {files.map((file, index) => (
                    <li key={index} className="grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-3 px-3 py-2">
                      <span className="wrap-anywhere font-mono">{file.name || path || <Missing />}</span>
                      <span className="text-kumo-subtle tabular-nums"><FileSize bytes={file.size_bytes} /></span>
                    </li>
                  ))}
                </ul>
              ) : <Missing />}
            </section>
          </div>
        </DialogBody>
        <DialogFooter>
          <Dialog.Close render={<Button variant="secondary" />}>{t("close")}</Dialog.Close>
        </DialogFooter>
      </Dialog>
    </Dialog.Root>
  );
}

function Detail({ title, children, className }: { title: string; children: ReactNode; className?: string }) {
  return (
    <div className={className}>
      <dt className="mb-0.5 text-kumo-subtle">{title}</dt>
      <dd>{children}</dd>
    </div>
  );
}
