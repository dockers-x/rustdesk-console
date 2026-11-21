import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import { Plus } from "@phosphor-icons/react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiPost, apiPut, http } from "../lib/api";
import { backupStatusKey, emptyStorageConfig, profilesKey, storageApi, useStorageProfiles, type StorageProfile, type StorageProfiles } from "../lib/storageProfiles";
import { StorageProfileFields } from "./StorageProfileFields";

export function StorageSettingsPage() {
  const { t } = useTranslation();
  const query = useStorageProfiles();
  const qc = useQueryClient();
  const [editor, setEditor] = useState<StorageProfile | "new">();
  const [archive, setArchive] = useState<StorageProfile>();
  const refresh = (data: StorageProfiles) => {
    qc.setQueryData(profilesKey, data);
    void qc.invalidateQueries({ queryKey: backupStatusKey });
  };
  const archiveProfile = useMutation({
    mutationFn: async (id: string) => await http.delete(`${storageApi}/profiles/${id}`) as unknown as StorageProfiles,
    onSuccess: (data) => { refresh(data); setArchive(undefined); },
  });

  return (
    <div className="mx-auto grid max-w-[96rem] min-w-0 gap-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold">{t("storageConfiguration")}</h1>
          <p className="mt-1 text-sm leading-6 text-kumo-subtle">{t("storageConfigurationHint")}</p>
        </div>
        <Button variant="primary" className="max-sm:min-h-11" onClick={() => setEditor("new")} disabled={!query.isSuccess}>
          <Plus size={16} aria-hidden />{t("storageAddProfile")}
        </Button>
      </header>
      {query.isPending && <TableState tone="loading">{t("loading")}</TableState>}
      {query.isError && <InlineMessage tone="error">{query.error.message}</InlineMessage>}
      {query.data?.entries.length === 0 && <TableState tone="empty">{t("storageNoProfiles")}</TableState>}
      {!!query.data?.entries.length && (
        <ul className="admin-surface divide-y divide-kumo-line rounded-lg border border-kumo-line bg-kumo-elevated">
          {query.data.entries.map((profile) => {
            const recording = query.data.recording === profile.id;
            const backup = query.data.backup?.profile_ids.includes(profile.id);
            return (
              <li key={profile.id} className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
                <div className="min-w-0">
                  <p className="font-medium [overflow-wrap:anywhere]">{profile.name}</p>
                  <div className="mt-1 flex flex-wrap gap-1.5">
                    <Badge variant="secondary">{profile.config.type === "local" ? t("recordStorageLocal") : profile.config.type === "s3" ? "S3" : "WebDAV"}</Badge>
                    {recording && <Badge variant="info">{t("recordFiles")}</Badge>}
                    {backup && <Badge variant="info">{t("automaticBackup")}</Badge>}
                    {profile.archived && <Badge variant="secondary">{t("storageArchived")}</Badge>}
                  </div>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button className="max-sm:min-h-11" variant="secondary" onClick={() => setEditor(profile)}>{t("edit")}</Button>
                  {!profile.archived && <Button className="max-sm:min-h-11" variant="ghost"
                    disabled={recording || backup}
                    onClick={() => { archiveProfile.reset(); setArchive(profile); }}>{t("storageArchive")}</Button>}
                </div>
              </li>
            );
          })}
        </ul>
      )}
      <div className="flex flex-wrap gap-2">
        <LinkButton variant="secondary" className="max-sm:min-h-11" href="#/record_file">{t("recordFiles")}</LinkButton>
        <LinkButton variant="secondary" className="max-sm:min-h-11" href="#/settings?tab=backups">{t("automaticBackup")}</LinkButton>
      </div>
      {editor && <ProfileEditor key={editor === "new" ? "new" : editor.id}
        profile={editor === "new" ? undefined : editor} close={() => setEditor(undefined)}
        saved={(data) => { refresh(data); setEditor(undefined); }} />}
      {archive && <ConfirmDialog open title={t("storageArchive")}
        description={t("storageArchiveDescription", { name: archive.name })}
        confirmLabel={t("storageArchive")} cancelLabel={t("cancel")}
        error={archiveProfile.error?.message} loading={archiveProfile.isPending}
        onConfirm={() => archiveProfile.mutate(archive.id)}
        onOpenChange={(open) => { if (!open) setArchive(undefined); }} />}
    </div>
  );
}

function ProfileEditor({ profile, close, saved }: {
  profile?: StorageProfile;
  close: () => void;
  saved: (data: StorageProfiles) => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(profile?.name ?? "");
  const [config, setConfig] = useState(profile?.config ?? emptyStorageConfig);
  const [allowHttp, setAllowHttp] = useState(profile?.allow_http ?? false);
  const save = useMutation({
    mutationFn: () => {
      const payload = { name, config, allow_http: allowHttp };
      return profile ? apiPut<StorageProfiles>(`${storageApi}/profiles/${profile.id}`, payload) :
        apiPost<StorageProfiles>(`${storageApi}/profiles`, payload);
    },
    onSuccess: saved,
  });

  return (
    <Dialog.Root open onOpenChange={(open) => { if (!open && !save.isPending) close(); }}>
      <Dialog size="lg" className={dialogPanelClass}>
        <DialogHeader title={t(profile ? "storageEditProfile" : "storageAddProfile")} />
        <form className="flex min-h-0 flex-col overflow-hidden"
          onSubmit={(event) => { event.preventDefault(); if (name.trim() && !save.isPending) save.mutate(); }}>
          <DialogBody>
            <div className="grid gap-4">
              <Input label={t("name")} value={name} maxLength={60} required onChange={(event) => setName(event.target.value)} />
              {profile && <p className="text-sm text-kumo-subtle">{t("storageFixedLocation")}</p>}
              <StorageProfileFields config={config} onChange={setConfig} locked={!!profile} />
              {config.type !== "local" && (
                <Switch label={t("storageAllowHttp")} checked={allowHttp} onCheckedChange={setAllowHttp} />
              )}
              {save.error && <InlineMessage tone="error">{save.error.message}</InlineMessage>}
            </div>
          </DialogBody>
          <DialogFooter>
            <Button type="button" variant="secondary" disabled={save.isPending} onClick={close}>{t("cancel")}</Button>
            <Button variant="primary" type="submit" loading={save.isPending} disabled={!name.trim()}>{t("save")}</Button>
          </DialogFooter>
        </form>
      </Dialog>
    </Dialog.Root>
  );
}
