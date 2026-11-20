import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Collapsible } from "@cloudflare/kumo/components/collapsible";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import { CaretDownIcon } from "@phosphor-icons/react";
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
  return (
    <Collapsible.Root className="mb-4 rounded-lg border border-kumo-line bg-kumo-base">
      <h2>
        <Collapsible.Trigger
          render={<Button variant="ghost" className="group h-auto min-h-11 w-full justify-between px-4 py-3 text-left sm:px-5" />}
        >
          <span className="whitespace-normal font-semibold">{t("auditRetention")}</span>
          <CaretDownIcon aria-hidden="true" size={16} className="shrink-0 text-kumo-subtle group-aria-expanded:rotate-180" />
        </Collapsible.Trigger>
      </h2>
      <Collapsible.Panel className="@container border-t border-kumo-line p-4 sm:p-5">
        <RetentionForm />
      </Collapsible.Panel>
    </Collapsible.Root>
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
  if (current.isPending) return <p className="text-sm" role="status">{t("loading")}</p>;
  return (
    <div className="space-y-4">
      {(current.isError || save.isError || preview.isError) && <InlineMessage tone="error">{t("operationFailed")}</InlineMessage>}
      {current.data && (
        <div className="grid gap-4 @3xl:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] @3xl:gap-6">
          <div className="min-w-0 space-y-4">
            <Switch label={t("auditEnableRetention")} checked={enabled} onCheckedChange={(value) => setDraft({ enabled: value, days })} />
            <div className="flex flex-wrap items-end gap-3">
              {enabled && (
                <div className="w-44 max-w-full">
                  <Input className="w-full" type="number" label={t("auditRetentionDays")} min={1} max={3650} value={days} onChange={(event) => setDraft({ enabled, days: event.target.value })} />
                </div>
              )}
              <Button variant="primary" disabled={!pending || (enabled && !validDays)} loading={save.isPending} onClick={() => save.mutate({ days: enabled ? Number(days) : null })}>{t("save")}</Button>
            </div>
          </div>
          <div className="min-w-0 space-y-3 border-t border-kumo-line pt-4 @3xl:border-t-0 @3xl:border-l @3xl:pt-0 @3xl:pl-6">
            <h3 className="text-sm font-semibold">{t("auditCleanup")}</h3>
            <p className="text-sm leading-relaxed text-kumo-subtle">{t("auditRetentionHint")}</p>
            {current.data.days !== null && !pending && (
              <div className="flex flex-wrap gap-2">
                <Button variant="secondary" loading={preview.isPending} onClick={() => preview.mutate()}>{t("auditPreview")}</Button>
                <Button variant="destructive" disabled={!hasCandidates || preview.isPending} onClick={() => { cleanup.reset(); setConfirm(true); }}>{t("auditCleanBatch")}</Button>
              </div>
            )}
            {batch?.cutoff && !pending && (
              <div className="space-y-2 rounded-lg bg-kumo-elevated p-3" role="status">
                <p className="text-sm font-medium">
                  {t(cleanup.data ? "auditRemovedBefore" : "auditEligibleBefore", {
                    cutoff: new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "medium" }).format(new Date(batch.cutoff)),
                  })}
                </p>
                <dl className="grid grid-cols-2 gap-3 border-t border-kumo-line pt-2 @5xl:grid-cols-4">
                  {LOGS.map(([key, label]) => (
                    <div key={key} className="min-w-0">
                      <dt className="text-xs break-words text-kumo-subtle">{t(label)}</dt>
                      <dd className="text-base font-semibold tabular-nums">{batch[key]}</dd>
                    </div>
                  ))}
                </dl>
              </div>
            )}
          </div>
        </div>
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
