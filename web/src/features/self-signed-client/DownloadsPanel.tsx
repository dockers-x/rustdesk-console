import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { InlineMessage } from "../../components/InlineMessage";
import { TableState } from "../../components/TableState";
import { apiGet, apiPost, apiPut, http } from "../../lib/api";
import { copyToClipboard } from "../../lib/clipboard";
import { featureError, selfSignedSettingsKey, type SigningSettings } from "./api";

const api = "/api/admin/client-downloads";
const portalsKey = ["client-download-portals"] as const;
const maxPackages = 16;
const platforms = { windows: "Windows", macos: "macOS", linux: "Linux" } as const;
const architectures = {
  x86_64: "x86_64",
  x86: "x86",
  aarch64: "ARM64",
  armv7: "ARMv7",
  riscv64: "RISC-V 64",
  universal: "",
} as const;

type Package = {
  platform: keyof typeof platforms;
  architecture: keyof typeof architectures;
  url: string;
  sha256: string;
};
type DraftPackage = { key: number; package: Package };
type Release = { version: string; packages: Package[] };
type Portal = Release & { id: number; path: string };

const panelClass = "min-w-0 rounded-lg border border-kumo-line bg-kumo-elevated p-4 sm:p-5";
const selectClass = "min-h-11 w-full min-w-0 rounded-md border border-kumo-line bg-kumo-base px-3 text-sm text-kumo-default focus-visible:outline-2 focus-visible:outline-kumo-brand";

function blankPackage(key: number): DraftPackage {
  return { key, package: { platform: "windows", architecture: "x86_64", url: "", sha256: "" } };
}

export function DownloadsPanel({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const save = useMutation({
    mutationFn: (next: boolean) => apiPut<{ enabled: boolean }>(api + "/settings", { enabled: next }),
    onSuccess: ({ enabled: next }) => {
      if (!next) queryClient.removeQueries({ queryKey: portalsKey });
      queryClient.setQueryData<SigningSettings>(selfSignedSettingsKey, (settings) =>
        settings && { ...settings, download_portal_enabled: next },
      );
    },
  });

  return (
    <div className="space-y-4">
      <section className={panelClass}>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-base font-semibold">{t("clientDownloadsTitle")}</h2>
            <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">{t("clientDownloadsToggleHint")}</p>
          </div>
          <Switch checked={enabled} disabled={save.isPending} label={t("clientDownloadsEnable")}
            onCheckedChange={(value) => save.mutate(value)} />
        </div>
        {save.isError && <InlineMessage tone="error" className="mt-3">{featureError(save.error)}</InlineMessage>}
      </section>
      {enabled && <PortalManager />}
    </div>
  );
}

function PortalManager() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const toasts = useKumoToastManager();
  const nextKey = useRef(1);
  const [version, setVersion] = useState("");
  const [packages, setPackages] = useState<DraftPackage[]>(() => [blankPackage(0)]);
  const [revoking, setRevoking] = useState<Portal>();
  const portals = useQuery({ queryKey: portalsKey, queryFn: () => apiGet<Portal[]>(api), gcTime: 0 });
  const publish = useMutation({
    mutationFn: (release: Release) => apiPost<Portal>(api, release),
    onSuccess: async () => {
      setVersion("");
      setPackages([blankPackage(nextKey.current++)]);
      await queryClient.invalidateQueries({ queryKey: portalsKey });
      toasts.add({ title: t("clientDownloadsPublished"), variant: "success" });
    },
  });
  const revoke = useMutation({
    mutationFn: (id: number) => http.delete(api + "/" + id),
    onSuccess: async () => {
      setRevoking(undefined);
      await queryClient.invalidateQueries({ queryKey: portalsKey });
      toasts.add({ title: t("clientDownloadsRevoked"), variant: "success" });
    },
  });

  const updatePackage = (key: number, changes: Partial<Package>) => {
    setPackages((drafts) => drafts.map((draft) =>
      draft.key === key ? { ...draft, package: { ...draft.package, ...changes } } : draft,
    ));
  };

  async function copyLink(portal: Portal) {
    const copied = await copyToClipboard(window.location.origin + portal.path);
    toasts.add({
      title: t(copied ? "copied" : "clientDownloadsCopyFailed"),
      variant: copied ? "success" : "error",
    });
  }

  return (
    <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
      <section className={panelClass}>
        <h3 className="text-base font-semibold">{t("clientDownloadsPublish")}</h3>
        <p className="mt-1 text-sm leading-6 text-kumo-subtle">{t("clientDownloadsSourceHint")}</p>
        <form className="mt-4 space-y-4" onSubmit={(event) => {
          event.preventDefault();
          publish.mutate({ version, packages: packages.map((draft) => draft.package) });
        }}>
          <fieldset disabled={publish.isPending} className="min-w-0 space-y-4">
            <Input label={t("clientDownloadsVersion")} value={version} required maxLength={64}
              placeholder="1.4.0" onChange={(event) => setVersion(event.target.value)} />
            {packages.map(({ key, package: entry }, index) => (
              <fieldset key={key} className="min-w-0 space-y-3 rounded-md border border-kumo-line p-3">
                <legend className="px-1 text-sm font-medium">{t("clientDownloadsPackage", { number: index + 1 })}</legend>
                <div className="grid min-w-0 gap-3 sm:grid-cols-2">
                  <label className="grid min-w-0 gap-1.5 text-sm font-medium">
                    {t("clientDownloadsPlatform")}
                    <select className={selectClass} value={entry.platform} onChange={(event) =>
                      updatePackage(key, { platform: event.target.value as Package["platform"] })}>
                      {Object.entries(platforms).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                    </select>
                  </label>
                  <label className="grid min-w-0 gap-1.5 text-sm font-medium">
                    {t("clientDownloadsArchitecture")}
                    <select className={selectClass} value={entry.architecture} onChange={(event) =>
                      updatePackage(key, { architecture: event.target.value as Package["architecture"] })}>
                      {Object.entries(architectures).map(([value, label]) => (
                        <option key={value} value={value}>{label || t("clientDownloadsUniversal")}</option>
                      ))}
                    </select>
                  </label>
                </div>
                <Input label={t("clientDownloadsUrl")} type="url" required maxLength={2048} value={entry.url}
                  onChange={(event) => updatePackage(key, { url: event.target.value })} />
                <Input label={t("clientDownloadsChecksum")} required maxLength={64} spellCheck={false}
                  value={entry.sha256} onChange={(event) => updatePackage(key, { sha256: event.target.value })} />
                {packages.length > 1 && (
                  <Button type="button" variant="ghost" className="min-h-11"
                    onClick={() => setPackages((drafts) => drafts.filter((draft) => draft.key !== key))}>
                    {t("clientDownloadsRemovePackage", { number: index + 1 })}
                  </Button>
                )}
              </fieldset>
            ))}
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="secondary" className="min-h-11" disabled={packages.length === maxPackages}
                onClick={() => {
                  const draft = blankPackage(nextKey.current++);
                  setPackages((current) => [...current, draft]);
                }}>{t("clientDownloadsAddPackage")}</Button>
              <Button type="submit" className="min-h-11" loading={publish.isPending}>{t("clientDownloadsPublish")}</Button>
            </div>
          </fieldset>
          {publish.isError && <InlineMessage tone="error">{featureError(publish.error)}</InlineMessage>}
        </form>
      </section>

      <section className={panelClass}>
        <h3 className="text-base font-semibold">{t("clientDownloadsActive")}</h3>
        {portals.isPending && <TableState tone="loading">{t("loading")}</TableState>}
        {portals.isError && <InlineMessage tone="error" className="mt-3">{featureError(portals.error)}</InlineMessage>}
        {portals.data?.length === 0 && <p className="mt-2 text-sm text-kumo-subtle">{t("clientDownloadsEmpty")}</p>}
        {!!portals.data?.length && (
          <ul className="mt-3 divide-y divide-kumo-line border-y border-kumo-line">
            {portals.data.map((portal) => (
              <li key={portal.id} className="space-y-2 py-3">
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <strong className="text-sm font-medium [overflow-wrap:anywhere]">{portal.version}</strong>
                  <span className="text-sm text-kumo-subtle">{t("clientDownloadsPackagesCount", { count: portal.packages.length })}</span>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <a href={portal.path} target="_blank" rel="noopener noreferrer"
                    className="inline-flex min-h-11 items-center rounded-sm px-2 text-sm font-medium text-kumo-brand underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-kumo-brand">
                    {t("clientDownloadsLink")}
                  </a>
                  <Button type="button" variant="secondary" className="min-h-11" onClick={() => void copyLink(portal)}>
                    {t("clientDownloadsCopy")}
                  </Button>
                  <Button type="button" variant="ghost" className="min-h-11" onClick={() => {
                    revoke.reset();
                    setRevoking(portal);
                  }}>{t("clientDownloadsRevoke")}</Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
      {revoking && (
        <ConfirmDialog open title={t("clientDownloadsRevoke")}
          description={t("clientDownloadsRevokeConfirm", { version: revoking.version })}
          confirmLabel={t("clientDownloadsRevoke")} cancelLabel={t("cancel")}
          loading={revoke.isPending} error={revoke.isError ? featureError(revoke.error) : undefined}
          onConfirm={() => revoke.mutate(revoking.id)}
          onOpenChange={(open) => { if (!open && !revoke.isPending) setRevoking(undefined); }} />
      )}
    </div>
  );
}
