import { useQuery } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Table } from "@cloudflare/kumo/components/table";
import {
  AddressBook,
  ChartBar,
  ClockCounterClockwise,
  Desktop,
  PlugsConnected,
  ShareNetwork,
  ShieldCheck,
  SignIn,
  Users,
} from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { TableState } from "../components/TableState";
import { apiGet } from "../lib/api";

interface Overview {
  generated_at: string;
  version: string;
  start_time: string;
  uptime_seconds: number;
  totals: {
    users: number;
    admins: number;
    devices: number;
    online_devices: number;
    active_connections: number;
    address_books: number;
    share_records: number;
    login_logs: number;
    audit_connections: number;
    audit_files: number;
  };
  platforms: { name: string; count: number }[];
  recent_logins: {
    id: number;
    user_id: number;
    client: string;
    ip: string;
    type: string;
    platform: string;
    created_at?: string | null;
  }[];
  recent_connections: {
    id: number;
    action: string;
    conn_id: number;
    peer_id: string;
    from_peer: string;
    ip: string;
    created_at?: string | null;
  }[];
}

export function OverviewPage() {
  const { t } = useTranslation();
  const overview = useQuery({
    queryKey: ["overview"],
    queryFn: () => apiGet<Overview>("/api/admin/overview"),
  });

  if (overview.isLoading) {
    return <TableState tone="loading">{t("loading")}</TableState>;
  }
  if (overview.error || !overview.data) {
    return (
      <TableState tone="error">
        {(overview.error as Error)?.message || t("operationFailed")}
      </TableState>
    );
  }

  const data = overview.data;
  const maxPlatform = Math.max(1, ...data.platforms.map((row) => row.count));

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("overview")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("overviewHint")}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge>{t("version")} {data.version}</Badge>
          <Button variant="secondary" onClick={() => void overview.refetch()}>
            {t("refresh")}
          </Button>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <MetricCard
          icon={<Users size={20} />}
          label={t("users")}
          value={data.totals.users}
          detail={`${data.totals.admins} ${t("admins")}`}
        />
        <MetricCard
          icon={<Desktop size={20} />}
          label={t("devices")}
          value={data.totals.devices}
          detail={`${data.totals.online_devices} ${t("onlineDevices")}`}
        />
        <MetricCard
          icon={<PlugsConnected size={20} />}
          label={t("activeConnections")}
          value={data.totals.active_connections}
          detail={`${data.totals.audit_connections} ${t("auditConn")}`}
        />
        <MetricCard
          icon={<AddressBook size={20} />}
          label={t("addressBook")}
          value={data.totals.address_books}
          detail={`${data.totals.share_records} ${t("shareRecords")}`}
        />
      </div>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="mb-4 flex items-center gap-3">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
              <ChartBar size={18} />
            </div>
            <div>
              <h2 className="text-base font-semibold">{t("platformBreakdown")}</h2>
              <p className="mt-1 text-sm text-kumo-subtle">
                {t("platformBreakdownHint")}
              </p>
            </div>
          </div>
          <div className="space-y-3">
            {data.platforms.length === 0 && (
              <TableState tone="empty">{t("noData")}</TableState>
            )}
            {data.platforms.map((row) => (
              <div key={row.name} className="grid gap-1">
                <div className="flex items-center justify-between gap-3 text-sm">
                  <span className="font-medium">{row.name}</span>
                  <span className="font-mono tabular-nums text-kumo-subtle">
                    {row.count}
                  </span>
                </div>
                <div className="h-2 overflow-hidden rounded-full bg-kumo-base">
                  <div
                    className="h-full rounded-full bg-kumo-brand"
                    style={{ width: `${Math.max(6, (row.count / maxPlatform) * 100)}%` }}
                  />
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="mb-4 flex items-center gap-3">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
              <ClockCounterClockwise size={18} />
            </div>
            <div>
              <h2 className="text-base font-semibold">{t("recentActivity")}</h2>
              <p className="mt-1 text-sm text-kumo-subtle">{t("recentActivityHint")}</p>
            </div>
          </div>
          <div className="grid gap-4 lg:grid-cols-2">
            <RecentLoginTable rows={data.recent_logins} />
            <RecentConnectionTable rows={data.recent_connections} />
          </div>
        </section>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <MetricCard
          icon={<SignIn size={20} />}
          label={t("loginLogs")}
          value={data.totals.login_logs}
          detail={t("recentLoginEvents")}
        />
        <MetricCard
          icon={<ShieldCheck size={20} />}
          label={t("auditFile")}
          value={data.totals.audit_files}
          detail={t("fileAuditEvents")}
        />
        <MetricCard
          icon={<ShareNetwork size={20} />}
          label={t("shareRecords")}
          value={data.totals.share_records}
          detail={t("webClientShares")}
        />
        <MetricCard
          icon={<ClockCounterClockwise size={20} />}
          label={t("uptime")}
          value={formatDuration(data.uptime_seconds)}
          detail={t("startedAt", { value: formatDate(data.start_time) })}
        />
      </div>
    </div>
  );
}

function MetricCard({
  icon,
  label,
  value,
  detail,
}: {
  icon: React.ReactNode;
  label: string;
  value: number | string;
  detail: string;
}) {
  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-sm font-medium text-kumo-subtle">{label}</p>
          <p className="mt-2 text-2xl font-semibold tabular-nums">{value}</p>
        </div>
        <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
          {icon}
        </div>
      </div>
      <p className="mt-3 truncate text-xs text-kumo-subtle" title={detail}>
        {detail}
      </p>
    </section>
  );
}

function RecentLoginTable({ rows }: { rows: Overview["recent_logins"] }) {
  const { t } = useTranslation();
  return (
    <div className="min-w-0 overflow-x-auto rounded-lg border border-kumo-line">
      <Table>
        <Table.Header>
          <Table.Row>
            <Table.Head>{t("loginLogs")}</Table.Head>
            <Table.Head>{t("ip")}</Table.Head>
            <Table.Head>{t("client")}</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {rows.map((row) => (
            <Table.Row key={row.id}>
              <Table.Cell>{row.user_id}</Table.Cell>
              <Table.Cell>{row.ip || "—"}</Table.Cell>
              <Table.Cell>{row.client || row.platform || "—"}</Table.Cell>
            </Table.Row>
          ))}
        </Table.Body>
      </Table>
      {rows.length === 0 && <TableState tone="empty">{t("noData")}</TableState>}
    </div>
  );
}

function RecentConnectionTable({ rows }: { rows: Overview["recent_connections"] }) {
  const { t } = useTranslation();
  return (
    <div className="min-w-0 overflow-x-auto rounded-lg border border-kumo-line">
      <Table>
        <Table.Header>
          <Table.Row>
            <Table.Head>{t("auditConn")}</Table.Head>
            <Table.Head>{t("deviceId")}</Table.Head>
            <Table.Head>{t("action")}</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {rows.map((row) => (
            <Table.Row key={row.id}>
              <Table.Cell>{row.conn_id || "—"}</Table.Cell>
              <Table.Cell>{row.peer_id || row.from_peer || "—"}</Table.Cell>
              <Table.Cell>{row.action || "—"}</Table.Cell>
            </Table.Row>
          ))}
        </Table.Body>
      </Table>
      {rows.length === 0 && <TableState tone="empty">{t("noData")}</TableState>}
    </div>
  );
}

function formatDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value || "—";
  return date.toLocaleString();
}

function formatDuration(seconds: number) {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}
