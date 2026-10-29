import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Navigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import { DownloadSimple, Key, ShieldCheck, Trash } from "@phosphor-icons/react";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { InlineMessage } from "../../components/InlineMessage";
import { TableState } from "../../components/TableState";
import { apiGet, apiPost, apiPut } from "../../lib/api";
import { BuildAssets } from "./BuildAssets";
import { Documentation } from "./Documentation";
import {
  downloadFile,
  emptyProfile,
  featureError,
  selfSignedBase,
  selfSignedEnabled,
  selfSignedProfileKey,
  selfSignedSettingsKey,
  useFeatures,
  type ClientCatalog,
  type ClientProfile,
  type SignedConfig,
  type SigningSettings,
} from "./api";

type ClientTab = "keys" | "branding" | "options" | "docs";
type KeyAction = "generate" | "import";
type Category = "config" | "local" | "display" | "builtin";

const fieldClass = "min-h-10 w-full min-w-0 rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm text-kumo-default focus:outline-none focus:ring-2 focus:ring-kumo-brand";
const restrictions = [
  ["disable-settings", "selfSignedDisableSettings"],
  ["disable-ab", "selfSignedDisableAb"],
  ["disable-account", "selfSignedDisableAccount"],
  ["disable-installation", "selfSignedDisableInstallation"],
  ["disable-tcp-listen", "selfSignedDisableTcp"],
] as const;

export function SelfSignedClientPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const features = useFeatures();
  const enabled = selfSignedEnabled(features.data);
  const [tab, setTab] = useState<ClientTab>("keys");
  const [profile, setProfile] = useState<ClientProfile>(emptyProfile);
  const [publicKey, setPublicKey] = useState("");
  const privateInput = useRef<HTMLTextAreaElement>(null);
  const [keyAction, setKeyAction] = useState<KeyAction | null>(null);
  const [assets, setAssets] = useState<Record<string, string>>({});
  const [message, setMessage] = useState("");
  const [localError, setLocalError] = useState("");
  const settings = useQuery({
    queryKey: selfSignedSettingsKey,
    queryFn: () => apiGet<SigningSettings>(selfSignedBase),
    enabled,
  });
  const savedProfile = useQuery({
    queryKey: selfSignedProfileKey,
    queryFn: () => apiGet<ClientProfile>(`${selfSignedBase}/profile`),
    enabled,
    refetchOnWindowFocus: false,
  });
  const catalog = useQuery({
    queryKey: ["self-signed-client-catalog"],
    queryFn: () => apiGet<ClientCatalog>(`${selfSignedBase}/catalog`),
    enabled,
  });

  useEffect(() => {
    if (savedProfile.data) setProfile(savedProfile.data);
  }, [savedProfile.data]);
  useEffect(() => {
    if (settings.data) setPublicKey(settings.data.public_key);
  }, [settings.data]);

  const saveKeys = useMutation({
    mutationFn: async (action: KeyAction) => {
      try {
        if (action === "generate") return await apiPost<SigningSettings>(`${selfSignedBase}/keys/generate`);
        const secret = privateInput.current?.value.trim() ?? "";
        if (!secret) throw new Error(t("selfSignedImportRequired"));
        return await apiPut<SigningSettings>(selfSignedBase, {
          enabled: true,
          private_key: secret,
          ...(publicKey.trim() ? { public_key: publicKey.trim() } : {}),
        });
      } catch (error) {
        throw new Error(featureError(error));
      }
    },
    onSuccess: (next) => {
      if (privateInput.current) privateInput.current.value = "";
      queryClient.setQueryData(selfSignedSettingsKey, next);
      setKeyAction(null);
      setMessage(t("selfSignedKeySaved"));
      setLocalError("");
    },
    gcTime: 0,
  });
  const saveProfile = useMutation({
    mutationFn: () => apiPut<ClientProfile>(`${selfSignedBase}/profile`, profile),
    onSuccess: (next) => {
      queryClient.setQueryData(selfSignedProfileKey, next);
      setMessage(t("selfSignedProfileSaved"));
      setLocalError("");
    },
    gcTime: 0,
  });
  const exportConfig = useMutation({
    mutationFn: () => apiPost<SignedConfig>(`${selfSignedBase}/sign`, profile),
    onSuccess: (artifact) => {
      downloadFile(artifact.filename, artifact.content);
      setMessage(t("selfSignedDownloadDone"));
      setLocalError("");
    },
    gcTime: 0,
  });

  if (features.isPending || (!enabled && features.isFetching)) return <TableState tone="loading">{t("loading")}</TableState>;
  if (features.isError) return <TableState tone="error">{featureError(features.error)}</TableState>;
  if (!enabled) return <Navigate to="/settings" replace />;

  const startKeyAction = (action: KeyAction) => {
    saveKeys.reset();
    setMessage("");
    setLocalError("");
    if (action === "import" && !privateInput.current?.value.trim()) {
      setLocalError(t("selfSignedImportRequired"));
      return;
    }
    if (settings.data?.key_configured) setKeyAction(action);
    else saveKeys.mutate(action);
  };
  const updateBranding = (key: string, value?: string) => {
    const next = { ...profile.branding };
    if (value === undefined) delete next[key];
    else next[key] = value;
    setProfile({ ...profile, branding: next });
  };
  const activeError = localError || [saveKeys, saveProfile, exportConfig]
    .find((mutation) => mutation.isError)?.error;
  const busy = saveKeys.isPending || saveProfile.isPending || exportConfig.isPending;
  const profileReady = Boolean(savedProfile.data && settings.data && catalog.data);

  return (
    <div className="space-y-5">
      <header className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold">{t("selfSignedClients")}</h1>
          <p className="mt-2 max-w-3xl text-sm leading-6 text-kumo-subtle">{t("selfSignedFeatureHint")}</p>
        </div>
        {tab !== "docs" && profileReady && (
          <div className="flex flex-wrap gap-2">
            <Button variant="secondary" disabled={busy} loading={saveProfile.isPending} onClick={() => saveProfile.mutate()}>{t("selfSignedSaveProfile")}</Button>
            <Button disabled={busy || !settings.data?.key_configured} loading={exportConfig.isPending} onClick={() => exportConfig.mutate()}><DownloadSimple size={16} />{t("selfSignedSign")}</Button>
          </div>
        )}
      </header>
      <nav aria-label={t("selfSignedClients")} className="grid grid-cols-2 gap-1 rounded-lg border border-kumo-line bg-kumo-elevated p-1 lg:grid-cols-4">
        {([['keys', 'selfSignedKeys'], ['branding', 'selfSignedBranding'], ['options', 'selfSignedOptions'], ['docs', 'selfSignedDocs']] as const).map(([value, label]) => (
          <button key={value} type="button" aria-current={tab === value ? "page" : undefined} onClick={() => { setTab(value); setMessage(""); setLocalError(""); }} className={`min-h-11 rounded-md px-3 py-2 text-sm font-medium focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand ${tab === value ? "bg-kumo-base text-kumo-default shadow-sm" : "text-kumo-subtle hover:bg-kumo-tint/60"}`}>{t(label)}</button>
        ))}
      </nav>
      {message && <InlineMessage tone="success">{message}</InlineMessage>}
      {Boolean(activeError) && <InlineMessage tone="error">{featureError(activeError)}</InlineMessage>}
      {tab === "docs" ? <Documentation /> : !profileReady ? (
        <TableState tone={savedProfile.isError || settings.isError || catalog.isError ? "error" : "loading"}>
          {savedProfile.error || settings.error || catalog.error ? featureError(savedProfile.error || settings.error || catalog.error) : t("selfSignedProfileLoading")}
        </TableState>
      ) : (
        <fieldset disabled={saveProfile.isPending} className="min-w-0">
          {tab === "keys" && (
            <Panel title={t("selfSignedKeyTitle")} hint={t("selfSignedKeyHint")}>
              <div className="flex items-center gap-2 text-sm font-medium"><ShieldCheck size={18} aria-hidden="true" />{t(settings.data?.key_configured ? "selfSignedKeyReady" : "selfSignedKeyMissing")}</div>
              <label className="block space-y-2 text-sm font-medium"><span>{t("selfSignedPublicKey")}</span><Input value={publicKey} onChange={(event) => setPublicKey(event.target.value)} className="w-full font-mono text-xs" autoComplete="off" spellCheck={false} /></label>
              {settings.data?.fingerprint && <div className="space-y-1"><p className="text-xs text-kumo-subtle">{t("selfSignedFingerprint")}</p><code className="block break-all text-xs">{settings.data.fingerprint}</code></div>}
              <label className="block space-y-2 text-sm font-medium"><span>{t("selfSignedPrivateKey")}</span><textarea ref={privateInput} className={`${fieldClass} min-h-20 font-mono text-xs`} autoComplete="off" spellCheck={false} rows={2} /></label>
              <p className="text-xs leading-5 text-kumo-subtle">{t("selfSignedPrivateHint")}</p>
              <div className="flex flex-wrap gap-2"><Button variant="secondary" disabled={busy} onClick={() => startKeyAction("import")}>{t("selfSignedImport")}</Button><Button disabled={busy} loading={saveKeys.isPending} onClick={() => startKeyAction("generate")}><Key size={16} />{t("selfSignedGenerate")}</Button></div>
              <p className="border-t border-kumo-line pt-4 text-sm leading-6 text-kumo-subtle">{t("selfSignedSignHint")}</p>
            </Panel>
          )}
          {tab === "branding" && (
            <div className="grid gap-5">
              <Panel title={t("selfSignedAppName")} hint={t("selfSignedAppNameHint")}>
                <Input aria-label={t("selfSignedAppName")} value={profile.app_name} maxLength={80} onChange={(event) => setProfile({ ...profile, app_name: event.target.value })} className="w-full max-w-lg" />
              </Panel>
              <BuildAssets assets={assets} onChange={setAssets} />
            </div>
          )}
          {tab === "options" && catalog.data && (
            <div className="grid gap-5">
              <Panel title={t("selfSignedHardSettings")} hint={t("selfSignedHardHint")}>
                <label className="block space-y-2 text-sm font-medium"><span>{t("selfSignedDirection")}</span><select value={profile.branding["conn-type"] ?? ""} onChange={(event) => updateBranding("conn-type", event.target.value || undefined)} className={`${fieldClass} max-w-md`}><option value="">{t("selfSignedBoth")}</option><option value="incoming">{t("selfSignedIncoming")}</option><option value="outgoing">{t("selfSignedOutgoing")}</option></select></label>
                <div className="grid gap-4 sm:grid-cols-2">{restrictions.filter(([key]) => catalog.data!.branding.includes(key)).map(([key, label]) => <Switch key={key} checked={profile.branding[key] === "Y"} label={t(label)} onCheckedChange={(next) => updateBranding(key, next ? "Y" : undefined)} />)}</div>
              </Panel>
              <div className="grid items-start gap-5 2xl:grid-cols-2">
                <OptionMap title={t("selfSignedDefaults")} hint={t("selfSignedDefaultsHint")} catalog={catalog.data} values={profile.default_settings} onChange={(next) => setProfile({ ...profile, default_settings: next })} />
                <OptionMap title={t("selfSignedOverrides")} hint={t("selfSignedOverridesHint")} catalog={catalog.data} values={profile.override_settings} onChange={(next) => setProfile({ ...profile, override_settings: next })} />
              </div>
              <p className="text-sm leading-6 text-kumo-subtle">{t("selfSignedExpertHint")}</p>
            </div>
          )}
        </fieldset>
      )}
      <ConfirmDialog open={keyAction !== null} title={t("selfSignedRotate")} description={t("selfSignedRotateHint")} confirmLabel={t("selfSignedRotate")} cancelLabel={t("cancel")} loading={saveKeys.isPending} error={saveKeys.isError ? featureError(saveKeys.error) : undefined} onOpenChange={(open) => { if (!open && !saveKeys.isPending) setKeyAction(null); }} onConfirm={() => { if (keyAction) saveKeys.mutate(keyAction); }} />
    </div>
  );
}

function Panel({ title, hint, children }: { title: string; hint: string; children: ReactNode }) {
  return <section className="min-w-0 space-y-4 rounded-lg border border-kumo-line bg-kumo-elevated p-4 sm:p-5"><div><h2 className="text-base font-semibold">{title}</h2><p className="mt-1 text-sm leading-6 text-kumo-subtle">{hint}</p></div>{children}</section>;
}

function OptionMap({ title, hint, catalog, values, onChange }: { title: string; hint: string; catalog: ClientCatalog; values: Record<string, string>; onChange: (next: Record<string, string>) => void }) {
  const { t } = useTranslation();
  const [category, setCategory] = useState<Category>("config");
  const [option, setOption] = useState("");
  const [value, setValue] = useState("");
  return (
    <Panel title={title} hint={hint}>
      <label className="block space-y-2 text-sm"><span>{t("selfSignedCategory")}</span><select className={fieldClass} value={category} onChange={(event) => { setCategory(event.target.value as Category); setOption(""); }}>{([['config', 'selfSignedConfigCategory'], ['local', 'selfSignedLocalCategory'], ['display', 'selfSignedDisplayCategory'], ['builtin', 'selfSignedBuiltinCategory']] as const).map(([key, label]) => <option key={key} value={key}>{t(label)}</option>)}</select></label>
      <label className="block space-y-2 text-sm"><span>{t("selfSignedOptionKey")}</span><select className={`${fieldClass} font-mono text-xs`} value={option} onChange={(event) => setOption(event.target.value)}><option value="">{t("selfSignedOptionKey")}</option>{catalog[category].map((key) => <option key={key} value={key}>{key}</option>)}</select></label>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end"><label className="block min-w-0 flex-1 space-y-2 text-sm"><span>{t("selfSignedOptionValue")}</span><Input value={value} onChange={(event) => setValue(event.target.value)} className="w-full font-mono text-xs" /></label><Button variant="secondary" disabled={!option} onClick={() => { onChange({ ...values, [option]: value }); setValue(""); }}>{t("selfSignedAddOption")}</Button></div>
      <p className="text-xs leading-5 text-kumo-subtle">{t("selfSignedStringHint")}</p>
      {Object.keys(values).length === 0 ? <p className="py-3 text-sm text-kumo-subtle">{t("selfSignedNoOptions")}</p> : (
        <div className="divide-y divide-kumo-line border-t border-kumo-line">
          {Object.entries(values).map(([key, item]) => <div key={key} className="flex min-w-0 items-center gap-2 py-3"><label className="min-w-0 flex-1 space-y-2"><span className="block break-all font-mono text-xs">{key}</span><Input aria-label={`${title}: ${key}`} value={item} onChange={(event) => onChange({ ...values, [key]: event.target.value })} className="w-full font-mono text-xs" /></label><Button variant="ghost" size="sm" aria-label={t("selfSignedRemoveOption", { key })} onClick={() => { const next = { ...values }; delete next[key]; onChange(next); }}><Trash size={16} /></Button></div>)}
        </div>
      )}
    </Panel>
  );
}
