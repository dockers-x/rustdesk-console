import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input, Textarea } from "@cloudflare/kumo/components/input";
import { Radio } from "@cloudflare/kumo/components/radio";
import { cn } from "@cloudflare/kumo/utils";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet, apiPatch, ApiError } from "../lib/api";

type SaveTarget = "local" | "server";

interface WebClientConfig {
  id_server: string;
  relay_server: string;
  api_server: string;
  key: string;
}

const LOCAL_OVERRIDE_KEY = "rustdesk-console.webclient.local-override";

const STORAGE_KEYS: Record<keyof WebClientConfig, string> = {
  id_server: "custom-rendezvous-server",
  relay_server: "relay-server",
  api_server: "api-server",
  key: "key",
};

const emptyConfig: WebClientConfig = {
  id_server: "",
  relay_server: "",
  api_server: "",
  key: "",
};

function toConfigString(value: unknown) {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return "";
}

function normalizeConfig(config: Partial<Record<keyof WebClientConfig, unknown>>): WebClientConfig {
  return {
    id_server: toConfigString(config.id_server),
    relay_server: toConfigString(config.relay_server),
    api_server: toConfigString(config.api_server),
    key: toConfigString(config.key),
  };
}

function hasLocalOverride() {
  return localStorage.getItem(LOCAL_OVERRIDE_KEY) === "1";
}

function readLocalConfig(server: WebClientConfig): WebClientConfig {
  const normalized = normalizeConfig(server);
  return {
    id_server:
      localStorage.getItem(STORAGE_KEYS.id_server) ??
      normalized.id_server,
    relay_server:
      localStorage.getItem(STORAGE_KEYS.relay_server) ??
      normalized.relay_server,
    api_server:
      localStorage.getItem(STORAGE_KEYS.api_server) ??
      normalized.api_server,
    key: localStorage.getItem(STORAGE_KEYS.key) ?? normalized.key,
  };
}

function writeWebClientOptions(config: WebClientConfig, localOverride: boolean) {
  for (const [field, key] of Object.entries(STORAGE_KEYS) as [
    keyof WebClientConfig,
    string,
  ][]) {
    const value = toConfigString(config[field]);
    localStorage.setItem(key, value);
    localStorage.setItem(`wc-${key}`, value);
  }
  if (localOverride) {
    localStorage.setItem(LOCAL_OVERRIDE_KEY, "1");
  } else {
    localStorage.removeItem(LOCAL_OVERRIDE_KEY);
  }
}

function valueOrEmpty(value: string, emptyLabel: string) {
  return value ? value : emptyLabel;
}

export function WebClientSettingsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [saveTarget, setSaveTarget] = useState<SaveTarget>("local");
  const [form, setForm] = useState<WebClientConfig>(emptyConfig);
  const [localActive, setLocalActive] = useState(false);
  const [message, setMessage] = useState("");
  const [localError, setLocalError] = useState("");

  const serverConfig = useQuery({
    queryKey: ["webclient-server-config"],
    queryFn: () => apiGet<WebClientConfig>("/api/admin/config/server"),
  });

  useEffect(() => {
    if (!serverConfig.data) return;
    const normalized = normalizeConfig(serverConfig.data);
    const active = hasLocalOverride();
    setLocalActive(active);
    setSaveTarget(active ? "local" : "server");
    setForm(active ? readLocalConfig(normalized) : normalized);
  }, [serverConfig.data]);

  const saveServer = useMutation({
    mutationFn: (payload: WebClientConfig) =>
      apiPatch<WebClientConfig>("/api/admin/config/server", payload),
    onSuccess: (saved) => {
      writeWebClientOptions(saved, false);
      setForm(saved);
      setLocalActive(false);
      setMessage(t("serverConfigSaved"));
      void qc.invalidateQueries({ queryKey: ["webclient-server-config"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setLocalError(ae.message || t("operationFailed"));
    },
  });

  const updateField = (field: keyof WebClientConfig, value: string) => {
    setForm((current) => ({ ...current, [field]: value }));
    setMessage("");
    setLocalError("");
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    setMessage("");
    setLocalError("");
    if (saveTarget === "local") {
      writeWebClientOptions(form, true);
      setLocalActive(true);
      setMessage(t("localOverrideSaved"));
      return;
    }
    saveServer.mutate(form);
  };

  const resetLocalOverride = () => {
    if (!serverConfig.data) return;
    const normalized = normalizeConfig(serverConfig.data);
    writeWebClientOptions(normalized, false);
    setForm(normalized);
    setSaveTarget("server");
    setLocalActive(false);
    setMessage(t("localOverrideCleared"));
    setLocalError("");
  };

  const effectiveConfig =
    serverConfig.data && localActive
      ? readLocalConfig(normalizeConfig(serverConfig.data))
      : serverConfig.data
        ? normalizeConfig(serverConfig.data)
        : undefined;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold">{t("webClientSettings")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("webClientSettingsHint")}
          </p>
        </div>
        <Button
          variant="secondary"
          loading={serverConfig.isFetching}
          onClick={() =>
            void qc.invalidateQueries({ queryKey: ["webclient-server-config"] })
          }
        >
          {t("refresh")}
        </Button>
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_340px]">
        <form
          onSubmit={submit}
          className="rounded-lg border border-kumo-line bg-kumo-elevated p-5"
        >
          <div className="mb-5">
            <h2 className="text-base font-semibold">{t("storageTarget")}</h2>
            <p className="mt-1 text-sm leading-6 text-kumo-subtle">
              {t("storageTargetHint")}
            </p>
          </div>

          <Radio.Group
            legend={t("storageTarget")}
            appearance="card"
            value={saveTarget}
            onValueChange={(value) => setSaveTarget(value as SaveTarget)}
            className="mb-5 grid gap-3 md:grid-cols-2"
          >
            <Radio.Item
              value="local"
              label={t("localStorageMode")}
              description={t("localStorageModeHint")}
            />
            <Radio.Item
              value="server"
              label={t("serverPersistentMode")}
              description={t("serverPersistentModeHint")}
            />
          </Radio.Group>

          <div className="grid gap-4 lg:grid-cols-2">
            <ConfigField
              label={t("apiServer")}
              storageKey={STORAGE_KEYS.api_server}
              value={form.api_server}
              onChange={(value) => updateField("api_server", value)}
              placeholder="https://api.example.com"
            />
            <ConfigField
              label={t("idServer")}
              storageKey={STORAGE_KEYS.id_server}
              value={form.id_server}
              onChange={(value) => updateField("id_server", value)}
              placeholder="id.example.com:21116"
            />
            <ConfigField
              label={t("relayServer")}
              storageKey={STORAGE_KEYS.relay_server}
              value={form.relay_server}
              onChange={(value) => updateField("relay_server", value)}
              placeholder="relay.example.com:21117"
            />
            <label className="block lg:row-span-2">
              <span className="mb-1 block text-sm font-medium">
                {t("publicKey")}
              </span>
              <Textarea
                aria-label={t("publicKey")}
                value={form.key}
                onChange={(e) => updateField("key", e.target.value)}
                className="min-h-28 font-mono text-xs"
                spellCheck={false}
                placeholder="OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw="
              />
              <span className="mt-1 block text-xs text-kumo-subtle">
                {STORAGE_KEYS.key}. {t("publicKeyHint")}
              </span>
            </label>
          </div>

          {message && (
            <InlineMessage tone="success" className="mt-5">
              {message}
            </InlineMessage>
          )}
          {(localError || serverConfig.error) && (
            <InlineMessage tone="error" className="mt-5">
              {localError ||
                (serverConfig.error as Error).message ||
                t("operationFailed")}
            </InlineMessage>
          )}

          <div className="mt-5 flex flex-wrap justify-end gap-2 border-t border-kumo-line pt-4">
            {localActive && (
              <Button
                type="button"
                variant="secondary"
                onClick={resetLocalOverride}
                disabled={!serverConfig.data}
              >
                {t("resetLocalOverride")}
              </Button>
            )}
            <Button
              type="submit"
              disabled={saveServer.isPending || serverConfig.isLoading}
              loading={saveServer.isPending}
            >
              {saveTarget === "local"
                ? t("writeLocalStorage")
                : t("writeServerConfig")}
            </Button>
          </div>
        </form>

        <aside className="space-y-4">
          <ConfigSummary
            title={t("currentEffectiveConfig")}
            config={effectiveConfig ?? emptyConfig}
            emptyLabel={t("emptyValue")}
          />
          <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
            <h2 className="text-base font-semibold">{t("browserOverride")}</h2>
            <dl className="mt-4 grid gap-3 text-sm">
              <SummaryRow
                label={t("status")}
                value={localActive ? t("active") : t("inactive")}
              />
              <SummaryRow
                label={t("serverDefaults")}
                value={
                  serverConfig.error
                    ? t("operationFailed")
                    : serverConfig.isLoading
                      ? t("loading")
                      : t("loaded")
                }
              />
            </dl>
          </section>
          {serverConfig.data && (
            <ConfigSummary
              title={t("currentServerConfig")}
              config={serverConfig.data}
              emptyLabel={t("emptyValue")}
            />
          )}
        </aside>
      </div>
    </div>
  );
}

function ConfigField({
  label,
  storageKey,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  storageKey: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
}) {
  return (
    <label className="block">
      <span className="mb-1 block text-sm font-medium">{label}</span>
      <Input
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        spellCheck={false}
      />
      <span className="mt-1 block text-xs text-kumo-subtle">{storageKey}</span>
    </label>
  );
}

function ConfigSummary({
  title,
  config,
  emptyLabel,
}: {
  title: string;
  config: WebClientConfig;
  emptyLabel: string;
}) {
  const { t } = useTranslation();
  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
      <h2 className="text-base font-semibold">{title}</h2>
      <dl className="mt-4 grid gap-3 text-sm">
        <SummaryRow
          label={t("apiServer")}
          value={valueOrEmpty(config.api_server, emptyLabel)}
        />
        <SummaryRow
          label={t("idServer")}
          value={valueOrEmpty(config.id_server, emptyLabel)}
        />
        <SummaryRow
          label={t("relayServer")}
          value={valueOrEmpty(config.relay_server, emptyLabel)}
        />
        <SummaryRow
          label={t("publicKey")}
          value={valueOrEmpty(config.key, emptyLabel)}
          muted={Boolean(config.key)}
        />
      </dl>
    </section>
  );
}

function SummaryRow({
  label,
  value,
  muted = false,
}: {
  label: string;
  value: string;
  muted?: boolean;
}) {
  return (
    <div className="grid gap-1">
      <dt className="text-xs font-medium text-kumo-subtle">{label}</dt>
      <dd
        className={cn(
          "break-all rounded-md border border-kumo-line bg-kumo-base px-2.5 py-2 font-mono text-xs leading-5",
          muted && "max-h-20 overflow-auto",
        )}
      >
        {value}
      </dd>
    </div>
  );
}
