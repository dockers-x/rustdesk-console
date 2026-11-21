import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { useTranslation } from "react-i18next";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet, apiPost } from "../lib/api";
import { backupStatusKey, storagePath } from "../lib/storageProfiles";
import { BackupStorageSelection } from "./BackupStorageSelection";
import { formatDateTime } from "../lib/dateFormat";

type Target = { name: string; kind: "s3" | "webdav"; outcome: "pending" | "uploaded" | "failed" | "interrupted"; detail?: string };
type Status = {
  sqlite: boolean;
  interval_hours: number;
  age_recipient: string;
  running: boolean;
  targets: Target[];
  last_run: {
    started_at: string;
    finished_at: string | null;
    archive: string;
    snapshot_failed: boolean;
    scheduled: boolean;
    targets: Target[];
  } | null;
};

const url = "/api/admin/backups";
const queryKey = backupStatusKey;
const protocols = { s3: "S3", webdav: "WebDAV" };

function TargetList({ targets, outcomes = false }: { targets: Target[]; outcomes?: boolean }) {
  const { t } = useTranslation();
  return (
    <ul className="divide-y divide-kumo-line rounded-md border border-kumo-line">
      {targets.map(({ name, kind, outcome, detail }) => (
        <li key={name} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2 text-sm">
          <span className="min-w-0 font-medium [overflow-wrap:anywhere]">{name}</span>
          <div className="flex items-center gap-2">
            <Badge variant="secondary">{protocols[kind]}</Badge>
            {outcomes && <span className={outcome === "failed" ? "text-kumo-danger" : "text-kumo-subtle"}>{t(`remoteBackup_${outcome}`)}</span>}
          </div>
          {outcomes && detail && <p className="basis-full text-sm text-kumo-subtle [overflow-wrap:anywhere]">{detail}</p>}
        </li>
      ))}
    </ul>
  );
}

export function RemoteBackupPanel({ timeZone }: { timeZone?: string }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const status = useQuery({
    queryKey,
    queryFn: () => apiGet<Status>(url),
    refetchInterval: (query) => query.state.data?.running ? 2000 : 15000,
  });
  const start = useMutation({
    mutationFn: () => apiPost<{ started: boolean }>(`${url}/run`),
    onSuccess: () => qc.invalidateQueries({ queryKey }),
  });
  const data = status.data;
  const run = data?.last_run;
  const sameTargets = run && data && data.targets.length === run.targets.length &&
    data.targets.every((target, index) => target.name === run.targets[index].name && target.kind === run.targets[index].kind);

  return (
    <div className="grid gap-4">
      {data?.sqlite && <BackupStorageSelection interval={data.interval_hours} recipient={data.age_recipient} />}
      <section id="remote-backup" className="grid min-w-0 gap-4 rounded-lg border border-kumo-line bg-kumo-elevated p-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-lg font-semibold">{t("remoteBackupTitle")}</h2>
            <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">{t("remoteBackupScope")}</p>
          </div>
          <Button type="button" className="max-sm:min-h-11" loading={start.isPending || data?.running}
            disabled={!data?.sqlite || !data.age_recipient || !data.targets.length || data.running || start.isPending}
            onClick={() => start.mutate()}>{t(data?.running ? "remoteBackupRunning" : "remoteBackupRun")}</Button>
        </div>
        {status.isPending && <p role="status" className="text-sm text-kumo-subtle">{t("loading")}</p>}
        {(status.isError || start.isError) && <InlineMessage tone="error">{t("remoteBackupError")}</InlineMessage>}
        {data && !data.sqlite && (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="text-sm text-kumo-subtle">{t("remoteBackupSqliteOnly")}</p>
            <LinkButton variant="secondary" href={`#${storagePath}`}>{t("storageConfiguration")}</LinkButton>
          </div>
        )}
        {data?.sqlite && !!data.targets.length && !data.age_recipient && (
          <p className="text-sm text-kumo-subtle">{t("backupEncryptionRequired")}</p>
        )}
        {data?.sqlite && (
          <>
            <p className="text-sm text-kumo-subtle">
              {data.targets.length ? t(data.age_recipient && data.interval_hours ? "remoteBackupSchedule" : "remoteBackupManual", { hours: data.interval_hours }) : t("remoteBackupSetup")}
            </p>
            {!!data.targets.length && !sameTargets && <TargetList targets={data.targets} />}
          </>
        )}
        {run && (
          <div className="grid gap-2 border-t border-kumo-line pt-4">
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <h3 className="font-medium">{t("remoteBackupLast")}</h3>
              <p role="status" className="text-sm text-kumo-subtle">
                {t(run.scheduled ? "remoteBackupScheduledRun" : "remoteBackupManualRun")} · {formatDateTime(run.started_at, timeZone)}
              </p>
            </div>
            {run.snapshot_failed && <InlineMessage tone="error">{t("remoteBackupSnapshotError")}</InlineMessage>}
            <TargetList targets={run.targets} outcomes />
            {run.targets.some((target) => target.outcome === "uploaded") && (
              <p className="text-xs text-kumo-subtle [overflow-wrap:anywhere]">{run.archive}</p>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
