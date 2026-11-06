import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { NavLink } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { apiGet, apiPost } from "../../lib/api";
import { getAuthSessionId } from "../../lib/auth";
import { environmentExample, sdkExamples } from "./examples";

interface Scope {
  id: string;
  label: string;
  admin_only: boolean;
}

interface KeyMetadata {
  id: number;
  name: string;
  prefix: string;
  scopes: string[];
  expires_at: number;
  revoked_at: number;
  created_at: number;
}

interface OpenApi {
  paths: Record<string, Record<string, {
    summary?: string;
    operationId?: string;
    description?: string;
    "x-required-scope"?: string;
    "x-admin-only"?: boolean;
  }>>;
}

const base = "/api/admin/developer";
const inputClass = "w-full rounded-lg border border-kumo-line bg-kumo-base px-3 py-2 text-sm text-kumo-default focus-visible:outline focus-visible:outline-2 focus-visible:outline-kumo-brand";
const cardClass = "rounded-xl border border-kumo-line bg-kumo-base p-5 sm:p-6";
const scopeMessages: Record<string, string> = {
  "profile:read": "developerScopeProfileRead",
  "devices:read": "developerScopeDevicesRead",
  "devices:write": "developerScopeDevicesWrite",
  "users:read": "developerScopeUsersRead",
  "users:write": "developerScopeUsersWrite",
  "device-groups:read": "developerScopeDeviceGroupsRead",
  "device-groups:write": "developerScopeDeviceGroupsWrite",
  "strategies:read": "developerScopeStrategiesRead",
  "address-books:read": "developerScopeAddressBooksRead",
  "address-books:write": "developerScopeAddressBooksWrite",
};

function CodeBlock({ code, label }: { code: string; label: string }) {
  const { t } = useTranslation();
  const [message, setMessage] = useState("");
  return (
    <div className="min-w-0">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium">{label}</h3>
        <Button size="sm" variant="secondary" onClick={() => {
          void navigator.clipboard?.writeText(code).then(
            () => setMessage(t("developerCopied")),
            () => setMessage(t("developerCopyFailed")),
          );
          if (!navigator.clipboard) setMessage(t("developerCopyFailed"));
        }}>{t("copy")}</Button>
      </div>
      <pre className="max-h-[32rem] overflow-auto rounded-lg border border-kumo-line bg-kumo-control p-4 text-xs leading-relaxed" tabIndex={0} aria-label={label}><code>{code}</code></pre>
      <p role="status" className="mt-1 text-xs text-kumo-subtle">{message}</p>
    </div>
  );
}

function ApiKeys({ isAdmin }: { isAdmin: boolean }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const session = getAuthSessionId();
  const keysQueryKey = ["developer-keys", session];
  const keys = useQuery({ queryKey: keysQueryKey, queryFn: () => apiGet<KeyMetadata[]>(`${base}/keys`) });
  const scopes = useQuery({ queryKey: ["developer-scopes", session], queryFn: () => apiGet<Scope[]>(`${base}/scopes`) });
  const [name, setName] = useState("");
  const [days, setDays] = useState(90);
  const [selected, setSelected] = useState(["profile:read", "devices:read", "address-books:read"]);
  const [busy, setBusy] = useState(false);
  const [hasSecret, setHasSecret] = useState(false);
  const [message, setMessage] = useState("");
  const secretRef = useRef<HTMLInputElement>(null);
  const visibleScopes = scopes.data?.filter((scope) => isAdmin || !scope.admin_only) ?? [];

  useEffect(() => {
    const field = secretRef.current;
    return () => { if (field) field.value = ""; };
  }, []);

  async function createKey(event: React.FormEvent) {
    event.preventDefault();
    if (!name.trim() || !selected.length) { setMessage(t("developerNameRequired")); return; }
    setBusy(true);
    setMessage("");
    try {
      const generated = await apiPost<{ key: string; metadata: KeyMetadata }>(`${base}/keys`, {
        name: name.trim(), scopes: selected, expires_at: Math.floor(Date.now() / 1000) + days * 86400,
      });
      if (secretRef.current) {
        secretRef.current.value = generated.key;
        setHasSecret(true);
      }
      setName("");
      void queryClient.invalidateQueries({ queryKey: keysQueryKey });
    } catch {
      setMessage(t("developerCreateFailed"));
    } finally { setBusy(false); }
  }

  async function revokeKey(key: KeyMetadata) {
    if (!window.confirm(t("developerRevokeConfirm", { name: key.name }))) return;
    setBusy(true);
    setMessage("");
    try {
      await apiPost(`${base}/keys/${key.id}/revoke`);
      setMessage(t("developerRevokedMessage"));
      void queryClient.invalidateQueries({ queryKey: keysQueryKey });
    } catch { setMessage(t("developerRevokeFailed")); }
    finally { setBusy(false); }
  }

  return (
    <div className="space-y-6">
      <p className="max-w-3xl text-sm leading-6 text-kumo-subtle">{t("developerKeyIntro")}</p>
      <section hidden={!hasSecret} className={`${cardClass} border-l-4 border-l-kumo-brand`} aria-labelledby="developer-once-title">
        <h2 id="developer-once-title" className="font-semibold">{t("developerOnce")}</h2>
        <p className="mt-2 max-w-3xl text-sm leading-6 text-kumo-subtle">{t("developerOnceHint")}</p>
        <label htmlFor="developer-new-key" className="mt-4 block text-sm font-medium">{t("developerSecret")}</label>
        <input id="developer-new-key" ref={secretRef} readOnly autoComplete="off" spellCheck={false} className={`${inputClass} mt-2 font-mono`} />
        <div className="mt-4 flex flex-wrap gap-2">
          <Button variant="secondary" onClick={() => {
            const value = secretRef.current?.value;
            if (!value) return;
            if (!navigator.clipboard) { setMessage(t("developerCopyFailed")); return; }
            void navigator.clipboard.writeText(value).then(() => setMessage(t("developerCopied")), () => setMessage(t("developerCopyFailed")));
          }}>{t("developerCopyKey")}</Button>
          <Button variant="secondary" onClick={() => {
            if (secretRef.current) secretRef.current.value = "";
            setHasSecret(false);
            setMessage("");
          }}>{t("developerDismissKey")}</Button>
        </div>
      </section>
      <form onSubmit={(event) => void createKey(event)} className={cardClass}>
        <fieldset disabled={busy || hasSecret || !scopes.data} className="space-y-5 disabled:opacity-60">
          <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_12rem]">
            <label className="space-y-2 text-sm font-medium"><span>{t("developerKeyName")}</span><input value={name} onChange={(event) => setName(event.target.value)} required maxLength={100} placeholder={t("developerKeyNameHint")} className={inputClass} /></label>
            <label className="space-y-2 text-sm font-medium"><span>{t("developerExpiry")}</span><select value={days} onChange={(event) => setDays(Number(event.target.value))} className={inputClass}>{[7, 30, 90, 365].map((count) => <option key={count} value={count}>{t("developerDays", { count })}</option>)}</select></label>
          </div>
          <div>
            <h2 className="text-sm font-medium">{t("developerScopes")}</h2>
            <div className="mt-3 grid gap-2 sm:grid-cols-2">
              {visibleScopes.map((scope) => <label key={scope.id} className="flex min-w-0 cursor-pointer items-start gap-3 rounded-lg border border-kumo-line p-3 text-sm hover:bg-kumo-control">
                <input type="checkbox" className="mt-1 size-4 shrink-0" checked={selected.includes(scope.id)} onChange={(event) => setSelected((current) => event.target.checked ? [...current, scope.id] : current.filter((value) => value !== scope.id))} />
                <span className="min-w-0"><span className="block">{t(scopeMessages[scope.id] ?? scope.label)}</span><code className="break-all text-xs text-kumo-subtle">{scope.id}</code>{scope.admin_only && <span className="ml-2 text-xs text-kumo-subtle">{t("developerAdminOnly")}</span>}</span>
              </label>)}
            </div>
          </div>
          <p className="text-xs leading-5 text-kumo-subtle">{t("developerSecureHint")}</p>
          <Button type="submit" disabled={!name.trim() || !selected.length}>{t(busy ? "developerCreating" : "developerGenerate")}</Button>
        </fieldset>
      </form>
      <p role="status" className="text-sm text-kumo-default">{message}</p>
      {(keys.isError || scopes.isError) && <div role="alert" className={cardClass}><p>{t("developerLoadFailed")}</p><Button className="mt-3" variant="secondary" onClick={() => { void keys.refetch(); void scopes.refetch(); }}>{t("developerRetry")}</Button></div>}
      <section className={cardClass}>
        <h2 className="font-semibold">{t("developerYourKeys")}</h2>
        {keys.isPending ? <p className="mt-4 text-sm" role="status">{t("loading")}</p> : !keys.data?.length ? <p className="mt-4 text-sm text-kumo-subtle">{t("developerKeyEmpty")}</p> : <ul className="mt-4 divide-y divide-kumo-line">
          {keys.data.map((key) => {
            const status = key.revoked_at > 0 ? "developerRevoked" : key.expires_at <= Date.now() / 1000 ? "developerExpired" : "developerActive";
            return <li key={key.id} className="flex flex-col gap-3 py-4 first:pt-0 last:pb-0 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0 space-y-1"><h3 className="break-words text-sm font-medium">{key.name}</h3><p className="break-all font-mono text-xs text-kumo-subtle">{key.prefix}…</p><p className="break-words text-xs text-kumo-subtle">{key.scopes.join(" · ")}</p><p className="text-xs text-kumo-subtle">{t("developerExpires")}: {new Date(key.expires_at * 1000).toLocaleString(i18n.language)} · {t(status)}</p></div>
              <Button variant="secondary" size="sm" disabled={busy || key.revoked_at > 0} onClick={() => void revokeKey(key)}>{t("developerRevoke")}</Button>
            </li>;
          })}
        </ul>}
      </section>
    </div>
  );
}

function Sdks() {
  const { t } = useTranslation();
  const [language, setLanguage] = useState<keyof typeof sdkExamples>("ts");
  const example = sdkExamples[language];
  return <div className="space-y-6">
    <p className="max-w-3xl text-sm leading-6 text-kumo-subtle">{t("developerSdkIntro")}</p>
    <section className={cardClass}><CodeBlock code={environmentExample} label={t("developerEnvironment")} /><p className="mt-2 text-xs leading-5 text-kumo-subtle">{t("developerEnvironmentHint")}</p></section>
    <section className={cardClass}>
      <div className="mb-5 flex flex-wrap gap-2" role="group" aria-label="SDK">{Object.entries(sdkExamples).map(([key, sdk]) => <Button key={key} variant={language === key ? "primary" : "secondary"} aria-pressed={language === key} onClick={() => setLanguage(key as keyof typeof sdkExamples)}>{sdk.label}</Button>)}</div>
      <div className="space-y-6"><CodeBlock code={example.install} label={t("developerRepository")} /><CodeBlock code={example.code} label={t("developerRunExample")} /><a className="inline-flex break-all text-sm text-kumo-link underline" href={example.repository} target="_blank" rel="noreferrer">{example.repository}</a></div>
    </section>
    <p className="text-sm leading-6 text-kumo-subtle">{t("developerSdkNote")}</p>
  </div>;
}

function OpenApiReference() {
  const { t } = useTranslation();
  const spec = useQuery({ queryKey: ["developer-openapi"], queryFn: () => apiGet<OpenApi>("/api/v1/openapi.json"), staleTime: 300000 });
  return <div className="space-y-6">
    <p className="max-w-3xl text-sm leading-6 text-kumo-subtle">{t("developerOpenapiIntro")}</p>
    <div className="flex flex-wrap gap-4 text-sm text-kumo-link underline"><a href="/api/v1/openapi.json" download="rustdesk-console-openapi.json">{t("developerDownloadSpec")}</a><a href="/api/v1/openapi.json" target="_blank" rel="noreferrer">{t("developerViewSpec")}</a></div>
    <section className={`${cardClass} space-y-4`}><h2 className="font-semibold">{t("developerAuth")}</h2><p className="text-sm leading-6 text-kumo-subtle">{t("developerAuthHint")}</p><p className="text-sm">{t("developerApiBase")}: <code>/api/v1</code></p><h2 className="font-semibold">{t("developerPagination")}</h2><p className="text-sm leading-6 text-kumo-subtle">{t("developerPaginationHint")}</p></section>
    <section className={cardClass}><h2 className="font-semibold">{t("developerOperations")}</h2>
      {spec.isPending && <p className="mt-4 text-sm" role="status">{t("loading")}</p>}
      {spec.isError && <div role="alert" className="mt-4 text-sm"><p>{t("developerLoadFailed")}</p><Button className="mt-3" variant="secondary" onClick={() => void spec.refetch()}>{t("developerRetry")}</Button></div>}
      <ul className="mt-4 divide-y divide-kumo-line">{Object.entries(spec.data?.paths ?? {}).flatMap(([path, methods]) => Object.entries(methods).filter(([method]) => ["get", "post", "patch", "put", "delete"].includes(method)).map(([method, operation]) => <li key={`${method}:${path}`} className="flex flex-col gap-2 py-3 sm:flex-row sm:items-start">
        <code className="shrink-0 text-xs font-semibold uppercase text-kumo-subtle sm:w-16">{method}</code>
        <div className="min-w-0 space-y-1">
          <code className="break-all text-sm">{path}</code>
          <p className="text-xs text-kumo-subtle">{operation.summary}</p>
          <p className="text-xs leading-5 text-kumo-subtle">{operation.description}</p>
          <p className="flex flex-wrap gap-2 text-xs"><code>{operation["x-required-scope"]}</code>{operation["x-admin-only"] && <span className="text-kumo-subtle">{t("developerAdminOnly")}</span>}</p>
        </div>
      </li>))}</ul>
    </section>
  </div>;
}

export function DeveloperPage({ section, isAdmin }: { section: "keys" | "sdks" | "openapi"; isAdmin: boolean }) {
  const { t } = useTranslation();
  return <div className="mx-auto w-full max-w-6xl space-y-6 p-4 sm:p-6 lg:p-8">
    <header><h1 className="text-2xl font-semibold tracking-tight">{t("developerAccess")}</h1><p className="mt-2 max-w-3xl text-sm leading-6 text-kumo-subtle">{t("developerIntro")}</p></header>
    <nav className="flex flex-wrap gap-2 border-b border-kumo-line pb-4" aria-label={t("developerAccess")}>{([ ["keys", "developerKeys"], ["sdks", "developerSdks"], ["openapi", "developerOpenapi"] ] as const).map(([path, label]) => <NavLink key={path} to={`/developer/${path}`} className={({ isActive }) => `rounded-lg px-3 py-2 text-sm font-medium ${isActive ? "bg-kumo-control text-kumo-default" : "text-kumo-subtle hover:bg-kumo-control"}`}>{t(label)}</NavLink>)}</nav>
    {section === "keys" ? <ApiKeys isAdmin={isAdmin} /> : section === "sdks" ? <Sdks /> : <OpenApiReference />}
  </div>;
}
