import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet, apiPost, apiPut } from "../lib/api";

const BASE = "/api/admin/console-audit";
const LOGS = [
  ["console", "consoleAudit"],
  ["login", "loginLogs"],
  ["connection", "auditConn"],
  ["file", "auditFile"],
] as const;
type Retention = { days: number | null };
type Batch = { cutoff: string | null } & Record<typeof LOGS[number][0], number>;

export function ConsoleAuditRetention() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <section className="mb-4 rounded-lg border border-kumo-line bg-kumo-base p-4" aria-label={t("auditRetention")}>
      <Button variant="secondary" aria-expanded={open} onClick={() => setOpen(!open)}>{t("auditRetention")}</Button>
      {open && <RetentionForm />}
    </section>
  );
}

function RetentionForm() {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const current = useQuery({ queryKey: [BASE, "retention"], queryFn: () => apiGet<Retention>(`${BASE}/retention`) });
  const [draft, setDraft] = useState<{ enabled: boolean; days: string }>();
  const [confirm, setConfirm] = useState(false);
  const enabled = draft?.enabled ?? current.data?.days != null;
  const days = draft?.days ?? String(current.data?.days ?? 90);
  const pending = draft !== undefined;
  const save = useMutation({
    mutationFn: (value: Retention) => apiPut(`${BASE}/retention`, value),
    onSuccess: (_, value) => {
      client.setQueryData([BASE, "retention"], value);
      preview.reset();
      cleanup.reset();
      setDraft(undefined);
    },
  });
  const preview = useMutation({
    mutationFn: () => apiGet<Batch>(`${BASE}/preview`),
    onMutate: () => cleanup.reset(),
  });
  const cleanup = useMutation({
    mutationFn: () => apiPost<Batch>(`${BASE}/cleanup`, { confirm: true }),
    onSuccess: () => {
      setConfirm(false);
      preview.reset();
      for (const name of ["console-audit", "login_log", "audit_conn", "audit_file"]) {
        void client.invalidateQueries({ queryKey: [name] });
      }
    },
  });
  const batch = cleanup.data ?? preview.data;
  const validDays = /^\d+$/.test(days) && Number(days) >= 1 && Number(days) <= 3650;
  const hasCandidates = preview.data && LOGS.some(([key]) => preview.data[key] > 0);
  if (current.isPending) return <p className="mt-4 text-sm" role="status">{t("loading")}</p>;
  return (
    <div className="mt-4 max-w-3xl space-y-4">
      <p className="text-sm text-kumo-subtle">{t("auditRetentionHint")}</p>
      {(current.isError || save.isError || preview.isError) && <InlineMessage tone="error">{t("operationFailed")}</InlineMessage>}
      {current.data && (
        <>
          <div className="flex flex-wrap items-end gap-4">
            <Switch label={t("auditEnableRetention")} checked={enabled} onCheckedChange={(value) => setDraft({ enabled: value, days })} />
            {enabled && (
              <Input type="number" label={t("auditRetentionDays")} min={1} max={3650} value={days} onChange={(event) => setDraft({ enabled, days: event.target.value })} />
            )}
            <Button variant="primary" disabled={!pending || (enabled && !validDays)} loading={save.isPending} onClick={() => save.mutate({ days: enabled ? Number(days) : null })}>{t("save")}</Button>
          </div>
          {current.data.days !== null && !pending && (
            <div className="flex flex-wrap gap-2">
              <Button variant="secondary" loading={preview.isPending} onClick={() => preview.mutate()}>{t("auditPreview")}</Button>
              <Button variant="destructive" disabled={!hasCandidates || preview.isPending} onClick={() => { cleanup.reset(); setConfirm(true); }}>{t("auditCleanBatch")}</Button>
            </div>
          )}
          {batch?.cutoff && !pending && (
            <div className="rounded-lg border border-kumo-line p-3" role="status">
              <p className="mb-2 text-sm font-medium">
                {t(cleanup.data ? "auditRemovedBefore" : "auditEligibleBefore", {
                  cutoff: new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "medium" }).format(new Date(batch.cutoff)),
                })}
              </p>
              <dl className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1 text-sm">
                {LOGS.map(([key, label]) => (
                  <div key={key} className="contents">
                    <dt>{t(label)}</dt><dd className="tabular-nums">{batch[key]}</dd>
                  </div>
                ))}
              </dl>
            </div>
          )}
        </>
      )}
      <ConfirmDialog
        open={confirm}
        title={t("auditCleanBatch")}
        description={t("auditCleanConfirm")}
        confirmLabel={t("delete")}
        cancelLabel={t("cancel")}
        onOpenChange={setConfirm}
        onConfirm={() => cleanup.mutate()}
        loading={cleanup.isPending}
        error={cleanup.isError ? t("operationFailed") : undefined}
      />
    </div>
  );
}
