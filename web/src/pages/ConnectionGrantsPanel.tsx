import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Select } from "@cloudflare/kumo/components/select";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet, apiPost, apiPut, http } from "../lib/api";

type Group = { id: number; name: string };
type Grant = { device_group_id: number };
type Decision = { allowed: boolean; reason: string };
type Action = { group: Group; revoke: boolean };

const base = "/api/admin/connection-grants";

function GroupSelect({ label, groups, value, change }: {
  label: string;
  groups: Group[];
  value: number | null;
  change: (value: number | null) => void;
}) {
  const { t } = useTranslation();
  return (
    <Select<number>
      label={label}
      placeholder={t("connectionGrantSelectGroup")}
      className="w-full max-sm:min-h-11"
      value={value}
      onValueChange={change}
      items={groups.map(({ id, name }) => ({ value: id, label: name }))}
      disabled={groups.length === 0}
    >
      {groups.map(({ id, name }) => <Select.Option key={id} value={id}>{name}</Select.Option>)}
    </Select>
  );
}

export function ConnectionGrantsPanel() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toasts = useKumoToastManager();
  const [source, setSource] = useState<number | null>(null);
  const [target, setTarget] = useState<number | null>(null);
  const [action, setAction] = useState<Action>();
  const [username, setUsername] = useState("");
  const [deviceId, setDeviceId] = useState("");

  const sources = useQuery({
    queryKey: [base, "user-groups"],
    queryFn: () => apiGet<{ list: Group[] }>("/api/admin/group/list", { page_size: 2147483647 }),
  });
  const targets = useQuery({
    queryKey: [base, "device-groups"],
    queryFn: () => apiGet<{ list: Group[] }>("/api/admin/device_group/list", { page_size: 2147483647 }),
  });
  const grants = useQuery({
    queryKey: [base, source],
    queryFn: () => apiGet<Grant[]>(`${base}/${source}`),
    enabled: source !== null,
  });
  const preview = useMutation({
    mutationFn: (form: { username: string; device_id: string }) => apiPost<Decision>(`${base}/preview`, form),
  });
  const save = useMutation({
    mutationFn: async ({ group, revoke }: Action) => {
      const url = `${base}/${source}/${group.id}`;
      if (revoke) await http.delete(url);
      else await apiPut(url);
    },
    onSuccess: () => {
      setAction(undefined);
      setTarget(null);
      preview.reset();
      void qc.invalidateQueries({ queryKey: [base, source] });
      toasts.add({ title: t("operationSuccess"), variant: "success" });
    },
  });
  const candidates = (targets.data?.list ?? []).filter((group) => !grants.data?.some((grant) => grant.device_group_id === group.id));

  return (
    <section className="grid min-w-0 gap-4 rounded-lg border border-kumo-line bg-kumo-elevated p-4">
      <div>
        <h2 className="text-lg font-semibold">{t("connectionGrants")}</h2>
        <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">{t("connectionGrantsHint")}</p>
      </div>
      {(sources.isError || targets.isError || grants.isError) && <InlineMessage tone="error">{t("operationFailed")}</InlineMessage>}
      {(sources.isPending || targets.isPending) && <p role="status" className="text-sm text-kumo-subtle">{t("loading")}</p>}
      <div className="grid items-end gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
        <GroupSelect label={t("connectionGrantSource")} groups={sources.data?.list ?? []} value={source}
          change={(id) => { setSource(id); setTarget(null); }} />
        <GroupSelect label={t("connectionGrantTarget")} groups={grants.isSuccess ? candidates : []} value={target} change={setTarget} />
        <Button type="button" variant="secondary" className="max-sm:min-h-11"
          disabled={source === null || target === null || !grants.isSuccess}
          onClick={() => {
            const group = candidates.find((candidate) => candidate.id === target);
            if (group) { save.reset(); setAction({ group, revoke: false }); }
          }}>{t("connectionGrantAdd")}</Button>
      </div>
      {source !== null && grants.isPending && <p role="status" className="text-sm text-kumo-subtle">{t("loading")}</p>}
      {grants.data?.length === 0 && <p className="text-sm text-kumo-subtle">{t("connectionGrantEmpty")}</p>}
      {!!grants.data?.length && (
        <ul className="divide-y divide-kumo-line rounded-md border border-kumo-line">
          {grants.data.map(({ device_group_id: id }) => {
            const group = targets.data?.list.find((entry) => entry.id === id) ?? { id, name: `#${id}` };
            return (
              <li key={id} className="flex flex-wrap items-center justify-between gap-2 px-3 py-1">
                <span className="min-w-0 text-sm [overflow-wrap:anywhere]">{group.name}</span>
                <Button type="button" variant="ghost" size="sm" className="max-sm:min-h-11"
                  aria-label={t("connectionGrantRevokeLabel", { name: group.name })}
                  onClick={() => { save.reset(); setAction({ group, revoke: true }); }}>{t("bookRevoke")}</Button>
              </li>
            );
          })}
        </ul>
      )}
      <form className="grid gap-3 border-t border-kumo-line pt-4"
        onSubmit={(event) => {
          event.preventDefault();
          if (username.trim() && deviceId.trim() && !preview.isPending) {
            preview.mutate({ username: username.trim(), device_id: deviceId.trim() });
          }
        }}>
        <h3 className="font-medium">{t("connectionGrantPreview")}</h3>
        <div className="grid items-end gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
          <Input label={t("username")} value={username} maxLength={128}
            onChange={(event) => { setUsername(event.target.value); preview.reset(); }} />
          <Input label={t("deviceId")} value={deviceId} maxLength={128}
            onChange={(event) => { setDeviceId(event.target.value); preview.reset(); }} />
          <Button type="submit" variant="secondary" className="max-sm:min-h-11" loading={preview.isPending}
            disabled={!username.trim() || !deviceId.trim()}>{t("connectionGrantCheck")}</Button>
        </div>
        {preview.data && <InlineMessage tone={preview.data.allowed ? "success" : "error"}>{t(`connectionDecision_${preview.data.reason}`)}</InlineMessage>}
        {preview.isError && <InlineMessage tone="error">{t("connectionGrantPreviewFailed")}</InlineMessage>}
      </form>
      {action && (
        <ConfirmDialog open destructive={action.revoke} title={t(action.revoke ? "bookRevoke" : "connectionGrantAdd")}
          description={t(action.revoke ? "connectionGrantRevokeConfirm" : "connectionGrantAddConfirm", {
            source: sources.data?.list.find((item) => item.id === source)?.name ?? "",
            target: action.group.name,
          })}
          cancelLabel={t("cancel")} confirmLabel={t(action.revoke ? "bookRevoke" : "connectionGrantAdd")}
          loading={save.isPending} error={save.isError ? t("operationFailed") : undefined}
          onConfirm={() => save.mutate(action)} onOpenChange={(open) => { if (!open) setAction(undefined); }} />
      )}
    </section>
  );
}
