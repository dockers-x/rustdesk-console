import { useId, useState } from "react";
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
type LogKind = typeof LOGS[number][0];
type LogDays = Record<LogKind, number | null>;
type Retention = { days: number | null; log_days?: LogDays; scheduled?: boolean };
type Batch = { cutoff: string | null; cutoffs?: Record<LogKind, string | null> } & Record<LogKind, number>;
type RetentionStatus = Retention & { last_run?: { at: string; result: Batch | null } };
type Draft = { days: Record<LogKind, string>; scheduled: boolean };

function draftFrom(settings: Retention): Draft {
  const daysFor = (key: LogKind) => String((settings.log_days ? settings.log_days[key] : settings.days) ?? "");
  return {
    days: {
      console: daysFor("console"),
      login: daysFor("login"),
      connection: daysFor("connection"),
      file: daysFor("file"),
    },
    scheduled: settings.scheduled ?? false,
  };
}

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
  const daysHint = useId();
  const scheduleHint = useId();
  const current = useQuery({
    queryKey: [BASE, "retention"],
    queryFn: () => apiGet<RetentionStatus>(`${BASE}/retention`),
    refetchInterval: (query) => query.state.data?.scheduled ? 30_000 : false,
  });
  const [draft, setDraft] = useState<Draft>();
  const [confirm, setConfirm] = useState(false);
  const settings = draft ?? (current.data && draftFrom(current.data));
  const save = useMutation({
    mutationFn: (value: Retention) => apiPut(`${BASE}/retention`, value),
    onSuccess: (_, value) => {
      client.setQueryData<RetentionStatus>([BASE, "retention"], (previous) => ({ ...previous, ...value }));
      void client.invalidateQueries({ queryKey: [BASE, "retention"] });
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
  const validDays = settings && Object.values(settings.days).every((value) =>
    value === "" || (/^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= 3650),
  );
  const hasRule = settings && Object.values(settings.days).some(Boolean);
  const hasCandidates = preview.data && LOGS.some(([key]) => preview.data[key] > 0);
  const dateFormat = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" });
  if (current.isPending) return <p className="text-sm" role="status">{t("loading")}</p>;
  return (
    <div className="space-y-4">
      {(current.isError || save.isError || preview.isError) && <InlineMessage tone="error">{t("operationFailed")}</InlineMessage>}
      {settings && (
        <div className="grid gap-4 @3xl:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] @3xl:gap-6">
          <form
            className="min-w-0 space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              if (!draft || !validDays || (draft.scheduled && !hasRule) || save.isPending) return;
              const daysFor = (key: LogKind) => draft.days[key] ? Number(draft.days[key]) : null;
              const unified = LOGS.every(([key]) => daysFor(key) === daysFor("console"));
              save.mutate({
                days: unified ? daysFor("console") : null,
                log_days: unified ? undefined : {
                  console: daysFor("console"),
                  login: daysFor("login"),
                  connection: daysFor("connection"),
                  file: daysFor("file"),
                },
                scheduled: draft.scheduled,
              });
            }}
          >
            <fieldset disabled={save.isPending} className="space-y-2" aria-describedby={daysHint}>
              <legend className="text-sm font-semibold">{t("auditRetentionDays")}</legend>
              <p id={daysHint} className="text-sm text-kumo-subtle">{t("auditRetentionDaysHint")}</p>
              <div className="grid gap-3 @sm:grid-cols-2">
                {LOGS.map(([key, label]) => (
                  <Input
                    key={key}
                    className="w-full"
                    type="number"
                    label={t(label)}
                    min={1}
                    max={3650}
                    value={settings.days[key]}
                    onChange={(event) =>
                      setDraft({ ...settings, days: { ...settings.days, [key]: event.target.value } })
                    }
                  />
                ))}
              </div>
            </fieldset>
            <div className="space-y-2">
              <Switch
                label={t("auditScheduleCleanup")}
                aria-describedby={scheduleHint}
                checked={settings.scheduled}
                disabled={save.isPending || (!hasRule && !settings.scheduled)}
                onCheckedChange={(scheduled) => setDraft({ ...settings, scheduled })}
              />
              <p id={scheduleHint} className="text-sm text-kumo-subtle">{t("auditScheduleHint")}</p>
              {settings.scheduled && !hasRule && (
                <p className="text-sm text-kumo-danger">{t("auditScheduleNeedsRule")}</p>
              )}
            </div>
            <Button
              type="submit"
              variant="primary"
              disabled={!draft || !validDays || (draft.scheduled && !hasRule)}
              loading={save.isPending}
            >
              {t("save")}
            </Button>
          </form>
          <div className="min-w-0 space-y-4 border-t border-kumo-line pt-4 @3xl:border-t-0 @3xl:border-l @3xl:pt-0 @3xl:pl-6">
            {current.data?.scheduled && (
              <div className="space-y-2">
                <h3 className="text-sm font-semibold">{t("auditScheduledCleanup")}</h3>
                {current.data.last_run ? (
                  <>
                    <p className="text-sm text-kumo-subtle">
                      {t(current.data.last_run.result ? "auditScheduledLastRun" : "auditScheduledFailed", {
                        time: dateFormat.format(new Date(current.data.last_run.at)),
                      })}
                    </p>
                    {current.data.last_run.result && (
                      <BatchSummary batch={current.data.last_run.result} dateFormat={dateFormat} removed />
                    )}
                  </>
                ) : (
                  <p className="text-sm text-kumo-subtle">{t("auditScheduledPending")}</p>
                )}
              </div>
            )}
            <div className="space-y-3">
              <h3 className="text-sm font-semibold">{t("auditCleanup")}</h3>
              <p className="text-sm text-kumo-subtle">{t("auditRetentionHint")}</p>
              {hasRule && !draft && (
                <div className="flex flex-wrap gap-2">
                  <Button variant="secondary" loading={preview.isPending} onClick={() => preview.mutate()}>{t("auditPreview")}</Button>
                  <Button variant="destructive" disabled={!hasCandidates || preview.isPending} onClick={() => { cleanup.reset(); setConfirm(true); }}>{t("auditCleanBatch")}</Button>
                </div>
              )}
              {batch && !draft && <BatchSummary batch={batch} dateFormat={dateFormat} removed={cleanup.data !== undefined} />}
            </div>
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

function BatchSummary({ batch, dateFormat, removed }: { batch: Batch; dateFormat: Intl.DateTimeFormat; removed: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2 rounded-lg bg-kumo-elevated p-3 text-sm" role="status">
      <p className="font-medium">{t(removed ? "auditRemovedRecords" : "auditEligibleRecords")}</p>
      <dl className="grid grid-cols-2 gap-3 border-t border-kumo-line pt-2">
        {LOGS.map(([key, label]) => {
          const cutoff = batch.cutoffs ? batch.cutoffs[key] : batch.cutoff;
          return (
            <div key={key} className="min-w-0">
              <dt className="text-kumo-subtle">{t(label)}</dt>
              <dd className="font-medium tabular-nums">{cutoff ? batch[key] : t("auditRetentionDisabled")}</dd>
              {cutoff && <dd className="text-kumo-subtle">{t("auditRetentionCutoff", { cutoff: dateFormat.format(new Date(cutoff)) })}</dd>}
            </div>
          );
        })}
      </dl>
    </div>
  );
}
