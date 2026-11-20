import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input, Textarea } from "@cloudflare/kumo/components/input";
import { Select } from "@cloudflare/kumo/components/select";
import { Switch } from "@cloudflare/kumo/components/switch";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { ApiError, apiGet, apiPost, apiPut } from "../lib/api";

type Scope = "base" | "one" | "subtree";
type IdentityAttribute = "entryUUID" | "objectGUID";

const scopes = {
  base: "ldapScopeBase",
  one: "ldapScopeOne",
  subtree: "ldapScopeSubtree",
} as const;

const userFields = {
  "base-dn": "ldapUserBaseDn",
  filter: "ldapFilter",
  username: "ldapUsernameAttribute",
  email: "ldapEmailAttribute",
  "first-name": "ldapFirstNameAttribute",
  "last-name": "ldapLastNameAttribute",
  "enable-attr": "ldapEnableAttribute",
  "enable-attr-value": "ldapEnableValue",
  "allow-group": "ldapAllowGroup",
  "admin-group": "ldapAlternateGroup",
} as const;

type UserField = keyof typeof userFields;

type Settings = {
  enable: boolean;
  url: string;
  "start-tls": boolean;
  "tls-verify": boolean;
  "base-dn": string;
  "bind-dn": string;
  user: Record<UserField, string> & {
    "identity-attribute": IdentityAttribute;
    scope: Scope;
    sync: boolean;
  };
};

type View = {
  settings: Settings;
  has_bind_password: boolean;
  has_file_ca: boolean;
  has_custom_ca: boolean;
};

const settingsUrl = "/api/admin/ldap/settings";
const queryKey = ["ldap-settings"] as const;

export function LdapSettingsPanel() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const config = useQuery({
    queryKey,
    queryFn: () => apiGet<View>(settingsUrl),
    staleTime: 0,
  });
  const [draft, setDraft] = useState<Settings>();
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const [ca, setCa] = useState("");
  const [clearCa, setClearCa] = useState(false);
  const [pending, setPending] = useState<"save" | "test">();
  const [notice, setNotice] = useState<{ tone: "error" | "success"; text: string }>();

  const settings = draft ?? config.data?.settings;
  if (config.isPending) return <TableState tone="loading">{t("loading")}</TableState>;
  if (!settings || !config.data) {
    return <TableState tone="error">{t("operationFailed")}</TableState>;
  }

  const view = config.data;
  const update = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    setDraft({ ...settings, [key]: value });
    setNotice(undefined);
  };
  const updateUser = <K extends keyof Settings["user"]>(key: K, value: Settings["user"][K]) =>
    update("user", { ...settings.user, [key]: value });

  async function submit(action: "save" | "test", values: Settings) {
    setPending(action);
    setNotice(undefined);
    const payload = {
      settings: {
        ...values,
        ...((clearCa || ca.trim()) ? { "tls-ca-pem": clearCa ? "" : ca.trim() } : {}),
      },
      ...((clearPassword || password) ? { bind_password: clearPassword ? "" : password } : {}),
    };
    try {
      if (action === "test") {
        await apiPost("/api/admin/ldap/test", payload);
      } else {
        client.setQueryData(queryKey, await apiPut<View>(settingsUrl, payload));
        setDraft(undefined);
        setPassword("");
        setClearPassword(false);
        setCa("");
        setClearCa(false);
      }
      setNotice({ tone: "success", text: t(action === "test" ? "ldapTestPassed" : "ldapSaved") });
    } catch (error) {
      setNotice({ tone: "error", text: error instanceof ApiError ? error.message : t("operationFailed") });
    } finally {
      setPending(undefined);
    }
  }

  function save(event: FormEvent) {
    event.preventDefault();
    if (!pending && settings) void submit("save", settings);
  }

  return (
    <form role="tabpanel" className="space-y-5" onSubmit={save}>
      <fieldset disabled={Boolean(pending)} className="min-w-0 space-y-5">
        <section className="space-y-4 rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <h2 className="font-semibold">{t("ldapConnection")}</h2>
              <p className="mt-1 text-sm text-kumo-subtle">{t("ldapConnectionHint")}</p>
            </div>
            <Switch label={t("ldapEnabled")} checked={settings.enable} onCheckedChange={(value) => update("enable", value)} />
          </div>
          <div className="grid gap-4 md:grid-cols-2">
            <Input label={t("ldapUrl")} value={settings.url} placeholder="ldaps://ldap.example.com:636" onChange={(event) => update("url", event.target.value)} />
            <Input label={t("ldapBaseDn")} value={settings["base-dn"]} onChange={(event) => update("base-dn", event.target.value)} />
            <Input label={t("ldapBindDn")} value={settings["bind-dn"]} onChange={(event) => update("bind-dn", event.target.value)} />
            <div className="space-y-2">
              <Input
                type="password"
                autoComplete="new-password"
                label={t("ldapBindPassword")}
                value={password}
                disabled={clearPassword || Boolean(pending)}
                onChange={(event) => {
                  setPassword(event.target.value);
                  setNotice(undefined);
                }}
              />
              <p className="text-sm text-kumo-subtle">
                {t(view.has_bind_password ? "ldapPasswordConfigured" : "ldapPasswordMissing")}
              </p>
              {view.has_bind_password && (
                <Switch label={t("ldapClearPassword")} checked={clearPassword} onCheckedChange={setClearPassword} />
              )}
            </div>
          </div>
          <div className="flex flex-wrap gap-x-6 gap-y-3">
            <Switch label={t("ldapStartTls")} checked={settings["start-tls"]} onCheckedChange={(value) => update("start-tls", value)} />
            <Switch label={t("ldapVerifyTls")} checked={settings["tls-verify"]} onCheckedChange={(value) => update("tls-verify", value)} />
          </div>
          <div className="space-y-2">
            <label htmlFor="ldap-ca" className="block text-sm font-medium">{t("ldapCustomCa")}</label>
            <Textarea
              id="ldap-ca"
              rows={3}
              className="w-full resize-y font-mono"
              placeholder="-----BEGIN CERTIFICATE-----"
              value={ca}
              disabled={clearCa || Boolean(pending)}
              onChange={(event) => {
                setCa(event.target.value);
                setNotice(undefined);
              }}
            />
            {view.has_file_ca && <p className="text-sm text-kumo-subtle">{t("ldapFileCa")}</p>}
            {view.has_custom_ca && (
              <Switch label={t("ldapClearCa")} checked={clearCa} onCheckedChange={setClearCa} />
            )}
          </div>
        </section>

        <section className="space-y-4 rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div>
            <h2 className="font-semibold">{t("ldapDirectoryUsers")}</h2>
            <p className="mt-1 text-sm text-kumo-subtle">{t("ldapDirectoryHint")}</p>
          </div>
          <div className="grid gap-4 md:grid-cols-2">
            <Select<Scope>
              label={t("ldapScope")}
              value={settings.user.scope}
              items={(Object.keys(scopes) as Scope[]).map((scope) => ({ value: scope, label: t(scopes[scope]) }))}
              disabled={Boolean(pending)}
              onValueChange={(scope) => { if (scope) updateUser("scope", scope); }}
            />
            <Select<IdentityAttribute>
              label={t("ldapIdentityAttribute")}
              value={settings.user["identity-attribute"]}
              items={[
                { value: "entryUUID", label: "entryUUID" },
                { value: "objectGUID", label: "objectGUID" },
              ]}
              disabled={Boolean(pending)}
              onValueChange={(value) => { if (value) updateUser("identity-attribute", value); }}
            />
            {(Object.keys(userFields) as UserField[]).map((key) => (
              <Input
                key={key}
                label={t(userFields[key])}
                value={settings.user[key]}
                onChange={(event) => updateUser(key, event.target.value)}
              />
            ))}
          </div>
          <Switch label={t("ldapSync")} checked={settings.user.sync} onCheckedChange={(value) => updateUser("sync", value)} />
        </section>
      </fieldset>
      {notice && <InlineMessage tone={notice.tone}>{notice.text}</InlineMessage>}
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="secondary" loading={pending === "test"} disabled={Boolean(pending)} onClick={() => void submit("test", settings)}>
          {t("ldapTest")}
        </Button>
        <Button type="submit" loading={pending === "save"} disabled={Boolean(pending)}>{t("ldapSave")}</Button>
      </div>
    </form>
  );
}
