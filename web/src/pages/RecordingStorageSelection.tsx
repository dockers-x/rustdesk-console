import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { Select } from "@cloudflare/kumo/components/select";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { InlineMessage } from "../components/InlineMessage";
import { apiPut } from "../lib/api";
import { profilesKey, storageApi, storagePath, useStorageProfiles, type StorageProfiles } from "../lib/storageProfiles";

export function RecordingStorageSelection() {
  const { t } = useTranslation();
  const profiles = useStorageProfiles();
  const qc = useQueryClient();
  const toasts = useKumoToastManager();
  const [draft, setDraft] = useState<string>();
  const save = useMutation({
    mutationFn: (id: string) => apiPut<StorageProfiles>(`${storageApi}/recording`, { profile_id: id || null }),
    onSuccess: (data) => {
      qc.setQueryData(profilesKey, data);
      setDraft(undefined);
      toasts.add({ title: t("operationSuccess"), variant: "success" });
    },
  });
  const saved = profiles.data?.recording ?? "";
  const selected = draft ?? saved;
  const items = [
    { value: "", label: t("storageExistingConfig") },
    ...(profiles.data?.entries ?? []).filter((profile) => !profile.archived)
      .map((profile) => ({ value: profile.id, label: profile.name })),
  ];

  return (
    <section className="mb-4 grid gap-3 rounded-lg border border-kumo-line bg-kumo-elevated p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="font-semibold">{t("recordStorage")}</h2>
          <p className="mt-1 text-sm text-kumo-subtle">{t("storageNewRecordingsHint")}</p>
        </div>
        <LinkButton href={`#${storagePath}`} variant="secondary" className="max-sm:min-h-11">{t("storageConfiguration")}</LinkButton>
      </div>
      <div className="grid gap-2 sm:grid-cols-[minmax(0,24rem)_auto] sm:items-end sm:justify-start">
        <Select<string> label={t("storageProfile")} items={items} value={selected}
          disabled={!profiles.isSuccess || save.isPending}
          className="w-full max-sm:min-h-11"
          onValueChange={(value) => { if (value !== null) setDraft(value); }}>
          {items.map(({ value, label }) => <Select.Option key={value} value={value}>{label}</Select.Option>)}
        </Select>
        <Button variant="secondary" className="max-sm:min-h-11" loading={save.isPending}
          disabled={!profiles.isSuccess || selected === saved} onClick={() => save.mutate(selected)}>{t("save")}</Button>
      </div>
      {(save.error || profiles.error) && <InlineMessage tone="error">{(save.error || profiles.error)?.message}</InlineMessage>}
    </section>
  );
}
