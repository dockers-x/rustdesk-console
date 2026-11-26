import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { apiGet } from "../lib/api";
import { getAuthSessionId } from "../lib/auth";
import { formatDateTime, formatUnixSeconds } from "../lib/dateFormat";
import { TableState } from "./TableState";

interface Recent<T> {
  entries: T[];
  has_more: boolean;
}

interface PresenceEvent {
  id: number;
  observed_at: number;
  online: boolean;
}

interface ConnectionReport {
  id: number;
  created_at: string;
  close_time: number;
  end_status: "closed" | "active" | "unknown";
  peer_id: string;
  from_peer: string;
}

interface Activity {
  presence: Recent<PresenceEvent>;
  connections: Recent<ConnectionReport>;
}

const PERIOD_DAYS = [7, 30, 90];

export function DeviceActivity({ rowId, peerId, timeZone }: {
  rowId: number;
  peerId: string;
  timeZone?: string;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [days, setDays] = useState(PERIOD_DAYS[0]);
  const activity = useQuery({
    queryKey: ["device-activity", getAuthSessionId(), rowId, peerId, days],
    enabled: expanded,
    queryFn: () => apiGet<Activity>(`/api/admin/peer/activity/${rowId}`, { days }),
  });

  const presence = activity.data?.presence;
  const connections = activity.data?.connections;

  return (
    <details
      className="mt-5 rounded-lg border border-kumo-line bg-kumo-base"
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      <summary className="min-h-11 cursor-pointer rounded-lg px-3 py-3 text-sm font-semibold focus-visible:outline-2 focus-visible:outline-kumo-brand">
        {t("deviceActivity")}
      </summary>
      {expanded && (
        <div className="grid gap-4 border-t border-kumo-line px-3 py-3 text-sm">
          <label className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span>{t("deviceActivityPeriod")}</span>
            <select
              className="min-h-11 rounded-md border border-kumo-line bg-kumo-base px-2 text-sm text-kumo-default focus-visible:outline-2 focus-visible:outline-kumo-brand sm:min-h-9"
              value={days}
              onChange={(event) => setDays(Number(event.target.value))}
            >
              {PERIOD_DAYS.map((period) => (
                <option key={period} value={period}>{t("deviceActivityDays", { count: period })}</option>
              ))}
            </select>
          </label>

          {activity.isPending && <TableState tone="loading">{t("loading")}</TableState>}
          {activity.isError && <TableState tone="error">{t("operationFailed")}</TableState>}
          {presence && connections && (
            <>
              <section className="grid gap-2">
                <h3 className="text-sm font-semibold">{t("deviceActivityPresence")}</h3>
                <p className="text-kumo-subtle">{t("deviceActivityPresenceHint")}</p>
                {presence.entries.length === 0 && (
                  <p className="text-kumo-subtle">{t("deviceActivityNoPresence")}</p>
                )}
                {presence.entries.length > 0 && (
                  <ol className="divide-y divide-kumo-line border-y border-kumo-line">
                    {presence.entries.map((entry) => (
                      <li key={entry.id} className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 py-2">
                        <span className="font-medium">{t(entry.online ? "online" : "deviceActivityTimeout")}</span>
                        <time className="tabular-nums text-kumo-subtle" dateTime={new Date(entry.observed_at * 1000).toISOString()}>
                          {formatUnixSeconds(entry.observed_at, timeZone)}
                        </time>
                      </li>
                    ))}
                  </ol>
                )}
                {presence.has_more && <p className="text-kumo-subtle">{t("deviceActivityMore", { count: presence.entries.length })}</p>}
              </section>

              <section className="grid gap-2">
                <h3 className="text-sm font-semibold">{t("auditConn")}</h3>
                {connections.entries.length === 0 && (
                  <p className="text-kumo-subtle">{t("deviceActivityNoConnections")}</p>
                )}
                {connections.entries.length > 0 && (
                  <ol className="divide-y divide-kumo-line border-y border-kumo-line">
                    {connections.entries.map((entry) => {
                      const incoming = entry.peer_id === peerId;
                      const otherId = (incoming ? entry.from_peer : entry.peer_id) || "—";
                      const end = entry.end_status === "closed"
                        ? t("deviceActivityEnded", { time: formatUnixSeconds(entry.close_time, timeZone) })
                        : t(entry.end_status === "active" ? "auditReportedActive" : "auditEndUnknown");
                      return (
                        <li key={entry.id} className="grid min-w-0 gap-1 py-2">
                          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
                            <span className="min-w-0 break-all font-medium">
                              {t(incoming ? "deviceActivityFrom" : "deviceActivityTo", { id: otherId })}
                            </span>
                            <time className="tabular-nums text-kumo-subtle" dateTime={`${entry.created_at.replace(" ", "T")}Z`}>
                              {formatDateTime(entry.created_at, timeZone)}
                            </time>
                          </div>
                          <span className="text-kumo-subtle">{end}</span>
                        </li>
                      );
                    })}
                  </ol>
                )}
                {connections.has_more && <p className="text-kumo-subtle">{t("deviceActivityMore", { count: connections.entries.length })}</p>}
              </section>
            </>
          )}
        </div>
      )}
    </details>
  );
}
