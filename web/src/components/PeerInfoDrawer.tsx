import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { X } from "@phosphor-icons/react";
import { Button } from "@cloudflare/kumo/components/button";
import { Table } from "@cloudflare/kumo/components/table";
import { useTranslation } from "react-i18next";
import { apiGet } from "../lib/api";
import { formatDateTime, formatUnixSeconds } from "../lib/dateFormat";
import { usePublicAdminConfig } from "../lib/adminTitle";
import { TableState } from "./TableState";

const PEER_ONLINE_WINDOW_SECONDS = 90;
const ACTIVE_CONNECTION_PAGE_SIZE = 50;

type PeerInfoScope = "admin" | "my";

interface ActiveConnection {
  id: number;
  conn_id: number;
  peer_id: string;
  uuid: string;
  created_at?: string | null;
  updated_at?: string | null;
}

interface ListResult<T> {
  list: T[];
  total: number;
  page: number;
  page_size: number;
}

function textValue(value: unknown) {
  const text = String(value ?? "").trim();
  return text || "—";
}

function numberValue(value: unknown) {
  const number = Number(value ?? 0);
  return Number.isFinite(number) ? number : 0;
}

function timestampValue(value: string) {
  return value === "0" ? "" : value;
}

function isPeerOnline(row: Record<string, unknown>) {
  const lastOnline = numberValue(row.last_online_time);
  return lastOnline > 0 && Date.now() / 1000 - lastOnline <= PEER_ONLINE_WINDOW_SECONDS;
}

function DetailItem({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: unknown;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0 rounded-md border border-kumo-line bg-kumo-base px-3 py-2">
      <dt className="text-xs font-medium text-kumo-subtle">{label}</dt>
      <dd
        className={`mt-1 break-words text-sm text-kumo-default ${
          mono ? "font-mono text-xs tabular-nums" : ""
        }`}
      >
        {textValue(value)}
      </dd>
    </div>
  );
}

function StatusPill({ online }: { online: boolean }) {
  const { t } = useTranslation();
  return (
    <span
      className={`inline-flex min-h-7 items-center gap-2 rounded-md border px-2.5 text-xs font-medium ${
        online
          ? "border-kumo-success/25 bg-kumo-success-tint/60 text-kumo-success"
          : "border-kumo-line bg-kumo-base text-kumo-subtle"
      }`}
    >
      <span
        className={`inline-flex size-2.5 rounded-full ${
          online ? "bg-kumo-success" : "bg-kumo-subtle/60"
        }`}
        aria-hidden="true"
      />
      {t(online ? "online" : "offline")}
    </span>
  );
}

export function PeerInfoDrawer({
  row,
  scope = "my",
}: {
  row: Record<string, unknown>;
  scope?: PeerInfoScope;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const adminConfig = usePublicAdminConfig();
  const displayTimeZone = adminConfig.data?.timezone?.trim() || undefined;
  const peerId = String(row.id ?? "");
  const uuid = String(row.uuid ?? "");
  const online = isPeerOnline(row);

  const activeConnections = useQuery({
    queryKey: ["peer-info-active-connections", peerId, uuid],
    enabled: open && scope === "admin" && Boolean(peerId || uuid),
    queryFn: () =>
      apiGet<ListResult<ActiveConnection>>("/api/admin/active_connection/list", {
        page: 1,
        page_size: ACTIVE_CONNECTION_PAGE_SIZE,
        peer_id: peerId,
        uuid,
      }),
  });

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);

  const heartbeatTime = useMemo(
    () => formatUnixSeconds(row.last_online_time, displayTimeZone),
    [displayTimeZone, row.last_online_time],
  );
  const sysinfoTime = useMemo(
    () => formatDateTime(row.updated_at, displayTimeZone),
    [displayTimeZone, row.updated_at],
  );

  return (
    <>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className="whitespace-nowrap"
        onClick={() => setOpen(true)}
      >
        {t("showDeviceInfo")}
      </Button>

      {open && (
        <div className="fixed inset-0 z-50">
          <button
            type="button"
            aria-label={t("close")}
            className="absolute inset-0 bg-black/35"
            onClick={() => setOpen(false)}
          />
          <aside
            role="dialog"
            aria-modal="true"
            aria-labelledby="peer-info-title"
            className="absolute inset-y-0 right-0 flex w-full max-w-xl flex-col border-l border-kumo-line bg-kumo-elevated text-kumo-default shadow-xl"
          >
            <div className="flex shrink-0 items-start justify-between gap-3 border-b border-kumo-line px-4 py-4 sm:px-5">
              <div className="min-w-0">
                <h2 id="peer-info-title" className="text-lg font-semibold">
                  {t("deviceSnapshot")}
                </h2>
                <p className="mt-1 break-all text-sm leading-6 text-kumo-subtle">
                  {peerId || textValue(row.hostname)}
                </p>
              </div>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                aria-label={t("close")}
                onClick={() => setOpen(false)}
              >
                <X size={16} />
              </Button>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto px-4 py-5 [scrollbar-gutter:stable] sm:px-5">
              <div className="mb-4 flex flex-wrap items-center gap-2">
                <StatusPill online={online} />
                <span className="rounded-md border border-kumo-warning/25 bg-kumo-warning-tint/30 px-2.5 py-1 text-xs font-medium text-kumo-warning">
                  {t("deviceSnapshotNotLiveMetrics")}
                </span>
              </div>

              <section className="grid gap-3">
                <h3 className="text-sm font-semibold">{t("deviceDataTime")}</h3>
                <dl className="grid gap-3 sm:grid-cols-2">
                  <DetailItem
                    label={t("lastHeartbeatAt")}
                    value={timestampValue(heartbeatTime)}
                  />
                  <DetailItem
                    label={t("sysinfoUpdatedAt")}
                    value={timestampValue(sysinfoTime)}
                  />
                  <DetailItem
                    label={t("createdAt")}
                    value={timestampValue(formatDateTime(row.created_at, displayTimeZone))}
                  />
                </dl>
              </section>

              <section className="mt-5 grid gap-3">
                <h3 className="text-sm font-semibold">{t("deviceIdentity")}</h3>
                <dl className="grid gap-3 sm:grid-cols-2">
                  <DetailItem label={t("deviceId")} value={peerId} mono />
                  <DetailItem label={t("uuid")} value={uuid} mono />
                  <DetailItem label={t("hostname")} value={row.hostname} />
                  <DetailItem label={t("alias")} value={row.alias} />
                  <DetailItem label={t("username")} value={row.username} />
                  <DetailItem label={t("ip")} value={row.last_online_ip} mono />
                </dl>
              </section>

              <section className="mt-5 grid gap-3">
                <h3 className="text-sm font-semibold">{t("hardwareSystem")}</h3>
                <dl className="grid gap-3 sm:grid-cols-2">
                  <DetailItem label={t("cpu")} value={row.cpu} />
                  <DetailItem label={t("memory")} value={row.memory} />
                  <DetailItem label={t("os")} value={row.os} />
                  <DetailItem label={t("version")} value={row.version} />
                  <DetailItem label={t("groupId")} value={row.group_id} mono />
                  <DetailItem label={t("userId")} value={row.user_id} mono />
                </dl>
                <p className="rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-xs leading-5 text-kumo-subtle">
                  {t("standardClientMetricsHint")}
                </p>
              </section>

              {scope === "admin" && (
                <section className="mt-5 grid gap-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <h3 className="text-sm font-semibold">{t("activeConnections")}</h3>
                    <span className="text-xs text-kumo-subtle">
                      {t("activeConnectionCount", {
                        count: activeConnections.data?.total ?? 0,
                      })}
                    </span>
                  </div>
                  <div className="overflow-x-auto rounded-lg border border-kumo-line">
                    <Table>
                      <Table.Header>
                        <Table.Row>
                          <Table.Head>{t("connId")}</Table.Head>
                          <Table.Head>{t("uuid")}</Table.Head>
                          <Table.Head>{t("updatedAt")}</Table.Head>
                        </Table.Row>
                      </Table.Header>
                      <Table.Body>
                        {(activeConnections.data?.list ?? []).map((conn) => (
                          <Table.Row key={conn.id}>
                            <Table.Cell>
                              <span className="font-mono text-xs tabular-nums">
                                {conn.conn_id}
                              </span>
                            </Table.Cell>
                            <Table.Cell>
                              <span className="block max-w-40 truncate font-mono text-xs">
                                {conn.uuid || "—"}
                              </span>
                            </Table.Cell>
                            <Table.Cell>
                              {formatDateTime(conn.updated_at, displayTimeZone)}
                            </Table.Cell>
                          </Table.Row>
                        ))}
                      </Table.Body>
                    </Table>
                    {activeConnections.isLoading && (
                      <TableState tone="loading">{t("loading")}</TableState>
                    )}
                    {activeConnections.error && (
                      <TableState tone="error">
                        {(activeConnections.error as Error).message ||
                          t("operationFailed")}
                      </TableState>
                    )}
                    {!activeConnections.isLoading &&
                      !activeConnections.error &&
                      (activeConnections.data?.list ?? []).length === 0 && (
                        <TableState tone="empty">{t("noData")}</TableState>
                      )}
                  </div>
                </section>
              )}
            </div>
          </aside>
        </div>
      )}
    </>
  );
}
