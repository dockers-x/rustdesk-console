import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input, Textarea } from "@cloudflare/kumo/components/input";
import { Radio } from "@cloudflare/kumo/components/radio";
import { cn } from "@cloudflare/kumo/utils";
import {
  CloudArrowUp,
  CopySimple,
  GlobeHemisphereWest,
  Key,
  PlugsConnected,
  TerminalWindow,
} from "@phosphor-icons/react";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet, apiPatch, apiPost, ApiError } from "../lib/api";

type SaveTarget = "local" | "server";
type WsMode = "auto" | "proxy" | "custom";

interface WebClientConfig {
  id_server: string;
  relay_server: string;
  api_server: string;
  ws_host: string;
  ws_id_host: string;
  ws_relay_host: string;
  key: string;
}

interface DeploymentCommandSet {
  linux: string[];
  macos: string[];
  windows: string[];
}

interface DeploymentConfig {
  encoded_config: string;
  filename_hint: string;
  config_command: DeploymentCommandSet;
  option_commands: DeploymentCommandSet;
  webclient_ws_routes: {
    id: string;
    relay: string;
  };
}

const LOCAL_OVERRIDE_KEY = "rustdesk-console.webclient.local-override";

const STORAGE_KEYS: Record<keyof WebClientConfig, string> = {
  id_server: "custom-rendezvous-server",
  relay_server: "relay-server",
  api_server: "api-server",
  ws_host: "ws-host",
  ws_id_host: "ws-id-host",
  ws_relay_host: "ws-relay-host",
  key: "key",
};

const emptyConfig: WebClientConfig = {
  id_server: "",
  relay_server: "",
  api_server: "",
  ws_host: "",
  ws_id_host: "",
  ws_relay_host: "",
  key: "",
};

function toConfigString(value: unknown) {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return "";
}

function normalizeConfig(
  config: Partial<Record<keyof WebClientConfig, unknown>>,
): WebClientConfig {
  return {
    id_server: toConfigString(config.id_server),
    relay_server: toConfigString(config.relay_server),
    api_server: toConfigString(config.api_server),
    ws_host: toConfigString(config.ws_host),
    ws_id_host: toConfigString(config.ws_id_host),
    ws_relay_host: toConfigString(config.ws_relay_host),
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
      localStorage.getItem(STORAGE_KEYS.id_server) ?? normalized.id_server,
    relay_server:
      localStorage.getItem(STORAGE_KEYS.relay_server) ?? normalized.relay_server,
    api_server:
      localStorage.getItem(STORAGE_KEYS.api_server) ?? normalized.api_server,
    ws_host: localStorage.getItem(STORAGE_KEYS.ws_host) ?? normalized.ws_host,
    ws_id_host:
      localStorage.getItem(STORAGE_KEYS.ws_id_host) ?? normalized.ws_id_host,
    ws_relay_host:
      localStorage.getItem(STORAGE_KEYS.ws_relay_host) ??
      normalized.ws_relay_host,
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

function detectWsMode(config: WebClientConfig): WsMode {
  if (config.ws_id_host.trim() || config.ws_relay_host.trim()) return "custom";
  if (config.ws_host.trim()) return "proxy";
  return "auto";
}

export function WebClientSettingsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [saveTarget, setSaveTarget] = useState<SaveTarget>("local");
  const [form, setForm] = useState<WebClientConfig>(emptyConfig);
  const [formReady, setFormReady] = useState(false);
  const [wsMode, setWsMode] = useState<WsMode>("auto");
  const [localActive, setLocalActive] = useState(false);
  const [message, setMessage] = useState("");
  const [localError, setLocalError] = useState("");

  const serverConfig = useQuery({
    queryKey: ["webclient-server-config"],
    queryFn: () => apiGet<WebClientConfig>("/api/admin/config/server"),
  });

  const deploymentPreview = useQuery({
    queryKey: ["webclient-deployment-preview", form],
    queryFn: () =>
      apiPost<DeploymentConfig>("/api/admin/config/deployment", form),
    enabled: formReady,
  });

  useEffect(() => {
    if (!serverConfig.data) return;
    const normalized = normalizeConfig(serverConfig.data);
    const active = hasLocalOverride();
    const nextForm = active ? readLocalConfig(normalized) : normalized;
    setLocalActive(active);
    setSaveTarget(active ? "local" : "server");
    setForm(nextForm);
    setWsMode(detectWsMode(nextForm));
    setFormReady(true);
  }, [serverConfig.data]);

  const saveServer = useMutation({
    mutationFn: (payload: WebClientConfig) =>
      apiPatch<WebClientConfig>("/api/admin/config/server", payload),
    onSuccess: (saved) => {
      const normalized = normalizeConfig(saved);
      writeWebClientOptions(normalized, false);
      setForm(normalized);
      setWsMode(detectWsMode(normalized));
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
    setWsMode(detectWsMode(normalized));
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
          loading={serverConfig.isFetching || deploymentPreview.isFetching}
          onClick={() => {
            void qc.invalidateQueries({ queryKey: ["webclient-server-config"] });
            void qc.invalidateQueries({
              queryKey: ["webclient-deployment-preview"],
            });
          }}
        >
          {t("refresh")}
        </Button>
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
        <form
          onSubmit={submit}
          className="rounded-lg border border-kumo-line bg-kumo-elevated"
        >
          <FormSection
            icon={<PlugsConnected size={18} />}
            title={t("connectionEndpoints")}
            description={t("connectionEndpointsHint")}
          >
            <div className="grid gap-4 lg:grid-cols-3">
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
            </div>
          </FormSection>

          <FormSection
            icon={<GlobeHemisphereWest size={18} />}
            title={t("webSocketRoutes")}
            description={t("webSocketRoutesHint")}
          >
            <WebSocketRouteEditor
              config={form}
              mode={wsMode}
              setMode={setWsMode}
              updateField={updateField}
              routes={deploymentPreview.data?.webclient_ws_routes}
            />
          </FormSection>

          <FormSection
            icon={<Key size={18} />}
            title={t("publicKey")}
            description={t("publicKeyHint")}
          >
            <label className="block">
              <Textarea
                aria-label={t("publicKey")}
                value={form.key}
                onChange={(e) => updateField("key", e.target.value)}
                className="min-h-28 font-mono text-xs"
                spellCheck={false}
                placeholder="OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw="
              />
              <span className="mt-1 block text-xs text-kumo-subtle">
                {STORAGE_KEYS.key}
              </span>
            </label>
          </FormSection>

          <FormSection
            icon={<CloudArrowUp size={18} />}
            title={t("storageTarget")}
            description={t("storageTargetHint")}
          >
            <Radio.Group
              legend={t("storageTarget")}
              appearance="card"
              value={saveTarget}
              onValueChange={(value) => setSaveTarget(value as SaveTarget)}
              className="grid gap-3 md:grid-cols-2"
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
          </FormSection>

          {(message || localError || serverConfig.error) && (
            <div className="px-5 pb-5">
              {message && (
                <InlineMessage tone="success">{message}</InlineMessage>
              )}
              {(localError || serverConfig.error) && (
                <InlineMessage tone="error">
                  {localError ||
                    (serverConfig.error as Error).message ||
                    t("operationFailed")}
                </InlineMessage>
              )}
            </div>
          )}

          <div className="flex flex-wrap justify-end gap-2 border-t border-kumo-line bg-kumo-recessed px-5 py-4">
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

        <aside className="space-y-4 xl:sticky xl:top-6 xl:self-start">
          <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
            <div className="flex items-start gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <GlobeHemisphereWest size={18} />
              </div>
              <div>
                <h2 className="text-base font-semibold">
                  {t("webClientRuntime")}
                </h2>
                <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                  {t("webClientRuntimeHint")}
                </p>
              </div>
            </div>
            <dl className="mt-4 grid gap-3 text-sm">
              <SummaryRow
                label={t("browserOverride")}
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

          <ConfigSummary
            title={t("currentEffectiveConfig")}
            config={effectiveConfig ?? emptyConfig}
            emptyLabel={t("emptyValue")}
          />

          <DeploymentPreview
            deployment={deploymentPreview.data}
            loading={
              serverConfig.isLoading ||
              !formReady ||
              (deploymentPreview.isPending &&
                deploymentPreview.fetchStatus !== "idle")
            }
            error={deploymentPreview.error as Error | null}
          />
        </aside>
      </div>
    </div>
  );
}

function FormSection({
  icon,
  title,
  description,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-kumo-line p-5 last:border-b-0">
      <div className="mb-4 flex items-start gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
          {icon}
        </div>
        <div>
          <h2 className="text-base font-semibold">{title}</h2>
          <p className="mt-1 text-sm leading-6 text-kumo-subtle">
            {description}
          </p>
        </div>
      </div>
      {children}
    </section>
  );
}

function ConfigField({
  label,
  storageKey,
  value,
  onChange,
  placeholder,
  helper,
}: {
  label: string;
  storageKey: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  helper?: string;
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
      <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
        {storageKey}
        {helper ? `. ${helper}` : ""}
      </span>
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
          label={t("wsHost")}
          value={valueOrEmpty(config.ws_host, emptyLabel)}
        />
        <SummaryRow
          label={t("wsIdHost")}
          value={valueOrEmpty(config.ws_id_host, emptyLabel)}
        />
        <SummaryRow
          label={t("wsRelayHost")}
          value={valueOrEmpty(config.ws_relay_host, emptyLabel)}
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

function WebSocketRouteEditor({
  config,
  mode,
  setMode,
  updateField,
  routes,
}: {
  config: WebClientConfig;
  mode: WsMode;
  setMode: (mode: WsMode) => void;
  updateField: (field: keyof WebClientConfig, value: string) => void;
  routes?: DeploymentConfig["webclient_ws_routes"];
}) {
  const { t } = useTranslation();
  const changeMode = (next: WsMode) => {
    setMode(next);
    if (next === "auto") {
      updateField("ws_host", "");
      updateField("ws_id_host", "");
      updateField("ws_relay_host", "");
      return;
    }
    if (next === "proxy") {
      updateField("ws_id_host", "");
      updateField("ws_relay_host", "");
      return;
    }
    updateField("ws_host", "");
  };
  return (
    <div className="grid gap-4">
      <Radio.Group
        legend={t("webSocketMode")}
        appearance="card"
        value={mode}
        onValueChange={(value) => changeMode(value as WsMode)}
        className="grid gap-3 lg:grid-cols-3"
      >
        <Radio.Item
          value="auto"
          label={t("webSocketModeAuto")}
          description={t("webSocketModeAutoHint")}
        />
        <Radio.Item
          value="proxy"
          label={t("webSocketModeProxy")}
          description={t("webSocketModeProxyHint")}
        />
        <Radio.Item
          value="custom"
          label={t("webSocketModeCustom")}
          description={t("webSocketModeCustomHint")}
        />
      </Radio.Group>

      {mode === "proxy" && (
        <ConfigField
          label={t("wsHost")}
          storageKey={STORAGE_KEYS.ws_host}
          value={config.ws_host}
          onChange={(value) => updateField("ws_host", value)}
          placeholder="https://rd.example.com"
          helper={t("wsHostHint")}
        />
      )}

      {mode === "custom" && (
        <div className="grid gap-4 lg:grid-cols-2">
          <ConfigField
            label={t("wsIdHost")}
            storageKey={STORAGE_KEYS.ws_id_host}
            value={config.ws_id_host}
            onChange={(value) => updateField("ws_id_host", value)}
            placeholder="wss://rd.example.com:21118"
            helper={t("wsIdHostHint")}
          />
          <ConfigField
            label={t("wsRelayHost")}
            storageKey={STORAGE_KEYS.ws_relay_host}
            value={config.ws_relay_host}
            onChange={(value) => updateField("ws_relay_host", value)}
            placeholder="wss://rd.example.com:21119"
            helper={t("wsRelayHostHint")}
          />
        </div>
      )}

      <div className="grid gap-4 rounded-md border border-kumo-line bg-kumo-base p-4 lg:grid-cols-2">
        <ReadonlyField
          label={t("idWebSocket")}
          value={routes?.id || t("derivedWebSocketRoutes")}
        />
        <ReadonlyField
          label={t("relayWebSocket")}
          value={routes?.relay || t("derivedWebSocketRoutes")}
        />
      </div>
    </div>
  );
}

function ReadonlyField({ label, value }: { label: string; value: string }) {
  return (
    <label className="block">
      <span className="mb-1 block text-sm font-medium text-kumo-subtle">
        {label}
      </span>
      <Input
        aria-label={label}
        value={value}
        readOnly
        className="font-mono text-sm"
        spellCheck={false}
      />
    </label>
  );
}

function DeploymentPreview({
  deployment,
  loading,
  error,
}: {
  deployment?: DeploymentConfig;
  loading: boolean;
  error?: Error | null;
}) {
  const { t } = useTranslation();
  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
      <div className="flex items-start gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
          <TerminalWindow size={18} />
        </div>
        <div>
          <h2 className="text-base font-semibold">{t("rustDeskDeployment")}</h2>
          <p className="mt-1 text-sm leading-6 text-kumo-subtle">
            {t("rustDeskDeploymentHint")}
          </p>
        </div>
      </div>
      {loading && !deployment && (
        <p
          role="status"
          className="mt-4 rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm text-kumo-subtle"
        >
          {t("loading")}
        </p>
      )}
      {error && (
        <InlineMessage tone="error" className="mt-4">
          {error.message || t("operationFailed")}
        </InlineMessage>
      )}
      {!loading && !deployment && !error && (
        <p
          role="status"
          className="mt-4 rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm text-kumo-subtle"
        >
          {t("emptyValue")}
        </p>
      )}
      {deployment && (
        <div className="mt-4 grid gap-3">
          <CopyField
            label={t("idWebSocket")}
            value={deployment.webclient_ws_routes.id}
          />
          <CopyField
            label={t("relayWebSocket")}
            value={deployment.webclient_ws_routes.relay}
          />
          <CopyField
            label={t("encodedConfig")}
            value={deployment.encoded_config}
          />
          <CopyField
            label={t("filenameHint")}
            value={deployment.filename_hint}
          />
          <CommandBlock
            title={t("configCommand")}
            commands={deployment.config_command}
          />
          <CommandBlock
            title={t("optionCommands")}
            commands={deployment.option_commands}
          />
        </div>
      )}
    </section>
  );
}

function CommandBlock({
  title,
  commands,
}: {
  title: string;
  commands: DeploymentCommandSet;
}) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-2">
      <p className="text-xs font-medium text-kumo-subtle">{title}</p>
      <CopyField label={t("linux")} value={commands.linux.join("\n")} />
      <CopyField label={t("macos")} value={commands.macos.join("\n")} />
      <CopyField label={t("windows")} value={commands.windows.join("\n")} />
    </div>
  );
}

function CopyField({ label, value }: { label: string; value: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const display = value || t("emptyValue");
  const copy = async () => {
    if (!value) return;
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };
  return (
    <div className="rounded-md border border-kumo-line bg-kumo-base p-2.5">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <span className="min-w-0 break-all text-xs font-medium text-kumo-subtle">
          {label}
        </span>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={copy}
          disabled={!value}
        >
          <CopySimple size={14} />
          {copied ? t("copied") : t("copy")}
        </Button>
      </div>
      <pre className="max-h-28 overflow-auto whitespace-pre-wrap break-all font-mono text-xs leading-5 text-kumo-default">
        {display}
      </pre>
    </div>
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
