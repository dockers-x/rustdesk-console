import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { Checkbox } from "@cloudflare/kumo/components/checkbox";
import { Input } from "@cloudflare/kumo/components/input";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { InlineMessage } from "../components/InlineMessage";
import { apiPut } from "../lib/api";
import { backupStatusKey, profilesKey, storageApi, storagePath, useStorageProfiles, type BackupSelection, type StorageProfiles } from "../lib/storageProfiles";

export function BackupStorageSelection({ interval, recipient }: { interval: number; recipient: string }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toasts = useKumoToastManager();
  const profiles = useStorageProfiles();
  const [selected, setSelected] = useState<string[]>();
  const [hours, setHours] = useState<string>();
  const [recipientDraft, setRecipientDraft] = useState<string>();
  const save = useMutation({
    mutationFn: (selection: BackupSelection | null) => apiPut<StorageProfiles>(`${storageApi}/backup`, { selection }),
    onSuccess: (data) => {
      qc.setQueryData(profilesKey, data);
      void qc.invalidateQueries({ queryKey: backupStatusKey });
      setSelected(undefined);
      setHours(undefined);
      setRecipientDraft(undefined);
      toasts.add({ title: t("operationSuccess"), variant: "success" });
    },
  });

  const saved = profiles.data?.backup;
  const ids = selected ?? saved?.profile_ids ?? [];
  const value = hours ?? String(saved?.interval_hours ?? (ids.length ? interval : 0));
  const parsed = Number(value);
  const ageRecipient = recipientDraft ?? saved?.age_recipient ?? recipient;
  const valid = value !== "" && Number.isInteger(parsed) && parsed >= 0 && parsed <= 720 &&
    (parsed === 0 || ids.length > 0) && ids.length <= 16 &&
    (ids.length === 0 || ageRecipient.trim().startsWith("age1"));
  const dirty = selected !== undefined || hours !== undefined || recipientDraft !== undefined;
  const options = (profiles.data?.entries ?? []).filter((profile) => !profile.archived && profile.config.type !== "local");

  return (
    <section className="grid min-w-0 gap-4 rounded-lg border border-kumo-line bg-kumo-elevated p-4">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">{t("automaticBackup")}</h2>
          <p className="mt-1 text-sm text-kumo-subtle">{t("backupStorageSelectionHint")}</p>
        </div>
        <LinkButton href={`#${storagePath}`} variant="secondary" className="max-sm:min-h-11">{t("storageConfiguration")}</LinkButton>
      </header>
      {profiles.isPending && <p role="status" className="text-sm text-kumo-subtle">{t("loading")}</p>}
      {profiles.isSuccess && (
        <form className="grid gap-4" onSubmit={(event) => {
          event.preventDefault();
          if (valid && dirty && !save.isPending) save.mutate({ profile_ids: ids, interval_hours: parsed, age_recipient: ageRecipient.trim() });
        }}>
          {!saved && <p className="text-sm text-kumo-subtle">{t("backupUsingExistingConfig")}</p>}
          <fieldset className="grid gap-2" disabled={save.isPending}>
            <legend className="mb-2 text-sm font-medium">{t("backupStorageTargets")}</legend>
            {options.length === 0 && <p className="text-sm text-kumo-subtle">{t("backupNoStorageProfiles")}</p>}
            {options.map((profile) => (
              <Checkbox key={profile.id}
                label={<span className="block max-sm:py-3">{profile.name}</span>}
                className="min-w-0 [overflow-wrap:anywhere]"
                checked={ids.includes(profile.id)}
                disabled={ids.length >= 16 && !ids.includes(profile.id)}
                onCheckedChange={(checked) => setSelected(checked ? [...ids, profile.id] : ids.filter((id) => id !== profile.id))}
              />
            ))}
          </fieldset>
          <div className="max-w-xs">
            <Input label={t("backupIntervalHours")} description={t("backupIntervalHint")}
              type="number" min={0} max={720} step={1} required value={value}
              disabled={save.isPending} onChange={(event) => setHours(event.target.value)} />
          </div>
          <Input label={t("backupAgeRecipient")} description={t("backupAgeRecipientHint")}
            placeholder="age1…" value={ageRecipient} maxLength={128}
            required={ids.length > 0} disabled={save.isPending}
            spellCheck={false} autoComplete="off"
            onChange={(event) => setRecipientDraft(event.target.value)} />
          {!valid && value !== "" && parsed > 0 && ids.length === 0 && <p className="text-sm text-kumo-subtle">{t("backupSelectTargetToSchedule")}</p>}
          <div className="flex flex-wrap gap-2">
            <Button type="submit" className="max-sm:min-h-11" loading={save.isPending} disabled={!valid || !dirty}>{t("save")}</Button>
            {saved && <Button type="button" variant="secondary" className="max-sm:min-h-11"
              disabled={save.isPending} onClick={() => save.mutate(null)}>{t("backupUseExistingConfig")}</Button>}
          </div>
        </form>
      )}
      {(save.error || profiles.error) && <InlineMessage tone="error">{(save.error || profiles.error)?.message}</InlineMessage>}
    </section>
  );
}
