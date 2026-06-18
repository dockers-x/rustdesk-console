import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as QRCode from "qrcode";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import { Tabs } from "@cloudflare/kumo/components/tabs";
import { cn } from "@cloudflare/kumo/utils";
import {
  CloudArrowUp,
  CopySimple,
  Eye,
  EyeSlash,
  GlobeHemisphereWest,
  PlugsConnected,
  QrCode,
  TerminalWindow,
} from "@phosphor-icons/react";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet, apiPatch, apiPost, ApiError } from "../lib/api";

type SaveTarget = "local" | "server";
type WsMode = "auto" | "proxy" | "custom";
type SettingsTab = "webclient" | "rustdesk" | "record-storage";
type DeploymentPlatform = keyof DeploymentCommandSet;
type RecordStorageType = "local" | "s3" | "webdav";
type ApproveMode = "" | "password" | "click" | "password-click";
type VerificationMethod =
  | ""
  | "use-temporary-password"
  | "use-permanent-password"
  | "use-both-passwords";

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
  mobile_config_text: string;
  mobile_config_uri: string;
  filename_hint: string;
  config_command: DeploymentCommandSet;
  option_commands: DeploymentCommandSet;
  webclient_ws_routes: {
    id: string;
    relay: string;
  };
}

type WebSocketRoutes = DeploymentConfig["webclient_ws_routes"];

interface RecordStorageConfig {
  type: RecordStorageType;
  local_dir: string;
  temp_dir: string;
  s3: {
    endpoint: string;
    region: string;
    bucket: string;
    prefix: string;
    access_key_id: string;
    secret_access_key: string;
    access_key_id_configured: boolean;
    secret_access_key_configured: boolean;
    force_path_style: boolean;
    clear_access_key_id?: boolean;
    clear_secret_access_key?: boolean;
  };
  webdav: {
    url: string;
    username: string;
    password: string;
    password_configured: boolean;
    prefix: string;
    clear_password?: boolean;
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

const emptyRecordStorageConfig: RecordStorageConfig = {
  type: "local",
  local_dir: "",
  temp_dir: "",
  s3: {
    endpoint: "",
    region: "auto",
    bucket: "",
    prefix: "record/",
    access_key_id: "",
    secret_access_key: "",
    access_key_id_configured: false,
    secret_access_key_configured: false,
    force_path_style: false,
    clear_access_key_id: false,
    clear_secret_access_key: false,
  },
  webdav: {
    url: "",
    username: "",
    password: "",
    password_configured: false,
    prefix: "record/",
    clear_password: false,
  },
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

function normalizeRecordStorageConfig(config: Partial<RecordStorageConfig>): RecordStorageConfig {
  const type =
    config.type === "s3" || config.type === "webdav" ? config.type : "local";
  return {
    type,
    local_dir: toConfigString(config.local_dir),
    temp_dir: toConfigString(config.temp_dir),
    s3: {
      endpoint: toConfigString(config.s3?.endpoint),
      region: toConfigString(config.s3?.region) || "auto",
      bucket: toConfigString(config.s3?.bucket),
      prefix: toConfigString(config.s3?.prefix) || "record/",
      access_key_id: toConfigString(config.s3?.access_key_id),
      secret_access_key: toConfigString(config.s3?.secret_access_key),
      access_key_id_configured: Boolean(config.s3?.access_key_id_configured),
      secret_access_key_configured: Boolean(config.s3?.secret_access_key_configured),
      force_path_style: Boolean(config.s3?.force_path_style),
      clear_access_key_id: Boolean(config.s3?.clear_access_key_id),
      clear_secret_access_key: Boolean(config.s3?.clear_secret_access_key),
    },
    webdav: {
      url: toConfigString(config.webdav?.url),
      username: toConfigString(config.webdav?.username),
      password: toConfigString(config.webdav?.password),
      password_configured: Boolean(config.webdav?.password_configured),
      prefix: toConfigString(config.webdav?.prefix) || "record/",
      clear_password: Boolean(config.webdav?.clear_password),
    },
  };
}

function storedConfigValues(field: keyof WebClientConfig) {
  const key = STORAGE_KEYS[field];
  return [localStorage.getItem(key), localStorage.getItem(`wc-${key}`)];
}

function readStoredConfigValue(field: keyof WebClientConfig, serverValue: string) {
  const [primary, secondary] = storedConfigValues(field);
  if (primary !== null && primary !== serverValue) return primary;
  if (secondary !== null && secondary !== serverValue) return secondary;
  return primary ?? secondary;
}

function hasLocalOverride() {
  return localStorage.getItem(LOCAL_OVERRIDE_KEY) === "1";
}

function readLocalConfig(server: WebClientConfig): WebClientConfig {
  const normalized = normalizeConfig(server);
  return {
    id_server:
      readStoredConfigValue("id_server", normalized.id_server) ??
      normalized.id_server,
    relay_server:
      readStoredConfigValue("relay_server", normalized.relay_server) ??
      normalized.relay_server,
    api_server:
      readStoredConfigValue("api_server", normalized.api_server) ??
      normalized.api_server,
    ws_host:
      readStoredConfigValue("ws_host", normalized.ws_host) ??
      normalized.ws_host,
    ws_id_host:
      readStoredConfigValue("ws_id_host", normalized.ws_id_host) ??
      normalized.ws_id_host,
    ws_relay_host:
      readStoredConfigValue("ws_relay_host", normalized.ws_relay_host) ??
      normalized.ws_relay_host,
    key: readStoredConfigValue("key", normalized.key) ?? normalized.key,
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

function normalizeWsBase(value: string) {
  const trimmed = value.trim().replace(/\/+$/g, "");
  if (!trimmed) return "";
  if (trimmed.startsWith("http://")) return `ws://${trimmed.slice(7)}`;
  if (trimmed.startsWith("https://")) return `wss://${trimmed.slice(8)}`;
  return trimmed;
}

function webSocketRoutesForConfig(config: WebClientConfig): WebSocketRoutes {
  const explicitId = normalizeWsBase(config.ws_id_host);
  const explicitRelay = normalizeWsBase(config.ws_relay_host);
  if (explicitId || explicitRelay) {
    return { id: explicitId, relay: explicitRelay };
  }
  const base = normalizeWsBase(config.ws_host);
  return {
    id: base ? `${base}/ws/id` : "",
    relay: base ? `${base}/ws/relay` : "",
  };
}

function detectWsMode(config: WebClientConfig): WsMode {
  if (config.ws_id_host.trim() || config.ws_relay_host.trim()) return "custom";
  if (config.ws_host.trim()) return "proxy";
  return "auto";
}

function configForWsMode(config: WebClientConfig, mode: WsMode): WebClientConfig {
  if (mode === "auto") {
    return {
      ...config,
      ws_host: "",
      ws_id_host: "",
      ws_relay_host: "",
    };
  }
  if (mode === "proxy") {
    return {
      ...config,
      ws_id_host: "",
      ws_relay_host: "",
    };
  }
  return {
    ...config,
    ws_host: "",
  };
}

export function WebClientSettingsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [saveTarget, setSaveTarget] = useState<SaveTarget>("local");
  const [form, setForm] = useState<WebClientConfig>(emptyConfig);
  const [recordStorageForm, setRecordStorageForm] = useState<RecordStorageConfig>(
    emptyRecordStorageConfig,
  );
  const [formReady, setFormReady] = useState(false);
  const [wsMode, setWsMode] = useState<WsMode>("auto");
  const [localActive, setLocalActive] = useState(false);
  const [message, setMessage] = useState("");
  const [localError, setLocalError] = useState("");
  const [activeTab, setActiveTab] = useState<SettingsTab>("webclient");
  const [deploymentPlatform, setDeploymentPlatform] =
    useState<DeploymentPlatform>("linux");
  const [deploymentPassword, setDeploymentPasswordState] = useState("");
  const [approveMode, setApproveMode] = useState<ApproveMode>("");
  const [verificationMethod, setVerificationMethod] =
    useState<VerificationMethod>("");
  const [deploymentPasswordRevision, setDeploymentPasswordRevision] = useState(0);
  const previewConfig = configForWsMode(form, wsMode);

  const serverConfig = useQuery({
    queryKey: ["webclient-server-config"],
    queryFn: () => apiGet<WebClientConfig>("/api/admin/config/server"),
    staleTime: 0,
    refetchOnMount: "always",
  });

  const recordStorageConfig = useQuery({
    queryKey: ["record-storage-config"],
    queryFn: () =>
      apiGet<RecordStorageConfig>("/api/admin/config/record-storage"),
    staleTime: 0,
    refetchOnMount: "always",
  });

  const deploymentPreview = useQuery({
    queryKey: [
      "webclient-deployment-preview",
      previewConfig,
      deploymentPasswordRevision,
    ],
    queryFn: () =>
      apiPost<DeploymentConfig>("/api/admin/config/deployment", {
        ...previewConfig,
        permanent_password: deploymentPassword,
        approve_mode: approveMode,
        verification_method: verificationMethod,
      }),
    enabled: formReady,
    gcTime: 0,
  });

  useEffect(() => {
    if (!recordStorageConfig.data) return;
    setRecordStorageForm(normalizeRecordStorageConfig(recordStorageConfig.data));
  }, [recordStorageConfig.data]);

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

  const saveRecordStorage = useMutation({
    mutationFn: (payload: RecordStorageConfig) =>
      apiPatch<RecordStorageConfig>("/api/admin/config/record-storage", payload),
    onSuccess: (saved) => {
      setRecordStorageForm(normalizeRecordStorageConfig(saved));
      setMessage(t("recordStorageSaved"));
      setLocalError("");
      void qc.invalidateQueries({ queryKey: ["record-storage-config"] });
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

  const updateRecordStorage = (next: RecordStorageConfig) => {
    setRecordStorageForm(next);
    setMessage("");
    setLocalError("");
  };

  const handleSaveTargetChange = (target: SaveTarget) => {
    setSaveTarget(target);
    setMessage("");
    setLocalError("");
    if (!serverConfig.data) return;

    const normalized = normalizeConfig(serverConfig.data);
    if (target === "server") {
      localStorage.removeItem(LOCAL_OVERRIDE_KEY);
      setForm(normalized);
      setWsMode(detectWsMode(normalized));
      setLocalActive(false);
      return;
    }

    const localConfig = readLocalConfig(normalized);
    setForm(localConfig);
    setWsMode(detectWsMode(localConfig));
    setLocalActive(hasLocalOverride());
  };

  const saveConfig = () => {
    setMessage("");
    setLocalError("");
    const payload = configForWsMode(form, wsMode);
    if (saveTarget === "local") {
      writeWebClientOptions(payload, true);
      setForm(payload);
      setLocalActive(true);
      setMessage(t("localOverrideSaved"));
      return;
    }
    saveServer.mutate(payload);
  };

  const setDeploymentPassword = (value: string) => {
    if (value.trim() && !deploymentPassword.trim() && !approveMode && !verificationMethod) {
      setApproveMode("password");
      setVerificationMethod("use-permanent-password");
    }
    setDeploymentPasswordState(value);
    setDeploymentPasswordRevision((current) => current + 1);
  };

  const updateApproveMode = (value: ApproveMode) => {
    setApproveMode(value);
    setDeploymentPasswordRevision((current) => current + 1);
  };

  const updateVerificationMethod = (value: VerificationMethod) => {
    setVerificationMethod(value);
    setDeploymentPasswordRevision((current) => current + 1);
  };

  const useUnattendedDefaults = () => {
    setApproveMode("password");
    setVerificationMethod("use-permanent-password");
    setDeploymentPasswordRevision((current) => current + 1);
  };

  const clearDeploymentOptions = () => {
    setDeploymentPasswordState("");
    setApproveMode("");
    setVerificationMethod("");
    setDeploymentPasswordRevision((current) => current + 1);
  };

  const saveRecordStorageConfig = () => {
    setMessage("");
    setLocalError("");
    saveRecordStorage.mutate(recordStorageForm);
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
  const effectiveWebSocketRoutes = effectiveConfig
    ? webSocketRoutesForConfig(effectiveConfig)
    : undefined;
  const deploymentLoading =
    serverConfig.isLoading ||
    !formReady ||
    (deploymentPreview.isPending && deploymentPreview.fetchStatus !== "idle");

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
          loading={
            serverConfig.isFetching ||
            deploymentPreview.isFetching ||
            recordStorageConfig.isFetching
          }
          onClick={() => {
            void qc.invalidateQueries({ queryKey: ["webclient-server-config"] });
            void qc.invalidateQueries({ queryKey: ["record-storage-config"] });
            void qc.invalidateQueries({
              queryKey: ["webclient-deployment-preview"],
            });
          }}
        >
          {t("refresh")}
        </Button>
      </div>

      <div className="space-y-4">
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated">
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-kumo-line px-4 py-3">
            <div className="flex min-w-0 items-center gap-3">
              <div className="flex size-8 shrink-0 items-center justify-center rounded-md border border-kumo-line bg-kumo-base text-kumo-brand">
                <PlugsConnected size={17} />
              </div>
              <div className="min-w-0">
                <h2 className="text-sm font-semibold">{t("clientBaseConfig")}</h2>
                <p className="truncate text-xs text-kumo-subtle">
                  {t("clientBaseConfigHint")}
                </p>
              </div>
            </div>
            <ConfigSaveActions
              localActive={localActive}
              serverConfigLoaded={Boolean(serverConfig.data)}
              saveTarget={saveTarget}
              setSaveTarget={handleSaveTargetChange}
              saving={saveServer.isPending}
              disabled={saveServer.isPending || serverConfig.isLoading}
              onResetLocal={resetLocalOverride}
              onSave={saveConfig}
            />
          </div>
          <div className="grid gap-3 p-4 md:grid-cols-2 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1.3fr)]">
            <ConfigField
              label={t("apiServer")}
              storageKey={STORAGE_KEYS.api_server}
              value={form.api_server}
              onChange={(value) => updateField("api_server", value)}
              placeholder="https://api.example.com"
              compact
            />
            <ConfigField
              label={t("idServer")}
              storageKey={STORAGE_KEYS.id_server}
              value={form.id_server}
              onChange={(value) => updateField("id_server", value)}
              placeholder="id.example.com:21116"
              compact
            />
            <ConfigField
              label={t("relayServer")}
              storageKey={STORAGE_KEYS.relay_server}
              value={form.relay_server}
              onChange={(value) => updateField("relay_server", value)}
              placeholder="relay.example.com:21117"
              compact
            />
            <SensitiveConfigField
              label={t("publicKey")}
              value={form.key}
              onChange={(value) => updateField("key", value)}
              placeholder="OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw="
              helper={t("publicKeyHint")}
              compact
            />
          </div>
        </section>

        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-kumo-line bg-kumo-elevated p-2">
          <TabButton
            active={activeTab === "webclient"}
            onClick={() => setActiveTab("webclient")}
          >
            {t("webClientSettingsTab")}
          </TabButton>
          <TabButton
            active={activeTab === "rustdesk"}
            onClick={() => setActiveTab("rustdesk")}
          >
            {t("rustDeskDeployment")}
          </TabButton>
          <TabButton
            active={activeTab === "record-storage"}
            onClick={() => setActiveTab("record-storage")}
          >
            {t("recordStorage")}
          </TabButton>
        </div>

        {activeTab === "webclient" && (
          <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
            <section className="rounded-lg border border-kumo-line bg-kumo-elevated">
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
            </section>

            <aside className="space-y-4 xl:sticky xl:top-6 xl:self-start">
              <RuntimeSummary
                localActive={localActive}
                serverLoading={serverConfig.isLoading}
                serverError={Boolean(serverConfig.error)}
              />
              <ConfigSummary
                title={t("currentEffectiveConfig")}
                config={effectiveConfig ?? emptyConfig}
                routes={effectiveWebSocketRoutes}
                emptyLabel={t("emptyValue")}
              />
            </aside>
          </div>
        )}

        {activeTab === "rustdesk" && (
          <DeploymentPreview
            deployment={deploymentPreview.data}
            loading={deploymentLoading}
            error={deploymentPreview.error as Error | null}
            platform={deploymentPlatform}
            setPlatform={setDeploymentPlatform}
            deploymentPassword={deploymentPassword}
            setDeploymentPassword={setDeploymentPassword}
            approveMode={approveMode}
            setApproveMode={updateApproveMode}
            verificationMethod={verificationMethod}
            setVerificationMethod={updateVerificationMethod}
            useUnattendedDefaults={useUnattendedDefaults}
            clearDeploymentOptions={clearDeploymentOptions}
          />
        )}

        {activeTab === "record-storage" && (
          <RecordStoragePanel
            config={recordStorageForm}
            loading={recordStorageConfig.isLoading}
            error={recordStorageConfig.error as Error | null}
            onChange={updateRecordStorage}
            saving={saveRecordStorage.isPending}
            saveDisabled={saveRecordStorage.isPending || recordStorageConfig.isLoading}
            onSave={saveRecordStorageConfig}
          />
        )}

        {(message || localError || serverConfig.error || recordStorageConfig.error) && (
          <div className="grid gap-2">
            {message && <InlineMessage tone="success">{message}</InlineMessage>}
            {(localError || serverConfig.error || recordStorageConfig.error) && (
              <InlineMessage tone="error">
                {localError ||
                  (recordStorageConfig.error as Error | undefined)?.message ||
                  (serverConfig.error as Error | undefined)?.message ||
                  t("operationFailed")}
              </InlineMessage>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function ConfigSaveActions({
  localActive,
  serverConfigLoaded,
  saveTarget,
  setSaveTarget,
  saving,
  disabled,
  onResetLocal,
  onSave,
}: {
  localActive: boolean;
  serverConfigLoaded: boolean;
  saveTarget: SaveTarget;
  setSaveTarget: (target: SaveTarget) => void;
  saving: boolean;
  disabled: boolean;
  onResetLocal: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-wrap items-center justify-end gap-2">
      <CompactButtonGroup
        ariaLabel={t("storageTarget")}
        value={saveTarget}
        onChange={setSaveTarget}
        options={[
          { value: "local", label: t("localStorageMode") },
          { value: "server", label: t("serverPersistentMode") },
        ]}
      />
      <div className="flex flex-wrap items-center gap-2">
        {localActive && (
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={onResetLocal}
            disabled={!serverConfigLoaded}
          >
            {t("resetLocalOverride")}
          </Button>
        )}
        <Button
          type="button"
          size="sm"
          onClick={onSave}
          disabled={disabled}
          loading={saving}
        >
          {saveTarget === "local" ? t("writeLocalStorage") : t("writeServerConfig")}
        </Button>
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "min-h-9 rounded-md border px-3 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand",
        active
          ? "border-kumo-brand bg-kumo-brand text-white"
          : "border-kumo-line bg-kumo-base text-kumo-subtle hover:bg-kumo-tint hover:text-kumo-default",
      )}
    >
      {children}
    </button>
  );
}

function CompactButtonGroup<T extends string>({
  ariaLabel,
  value,
  options,
  onChange,
  className,
}: {
  ariaLabel: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  className?: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className={cn(
        "inline-flex min-h-8 flex-wrap items-center gap-1 rounded-md border border-kumo-line bg-kumo-base p-1",
        className,
      )}
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={`${ariaLabel}-${option.value || "empty"}`}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => onChange(option.value)}
            className={cn(
              "min-h-6 rounded px-2 text-xs font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand",
              selected
                ? "bg-kumo-brand text-white"
                : "text-kumo-subtle hover:bg-kumo-tint hover:text-kumo-default",
            )}
          >
            {option.label}
          </button>
        );
      })}
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
  compact = false,
}: {
  label: string;
  storageKey: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  helper?: string;
  compact?: boolean;
}) {
  return (
    <label className="block min-w-0">
      <span className="mb-1 block text-sm font-medium">{label}</span>
      <Input
        aria-label={label}
        size={compact ? "sm" : "base"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        spellCheck={false}
        className="w-full min-w-0"
      />
      {!compact && (
        <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
          {storageKey}
          {helper ? `. ${helper}` : ""}
        </span>
      )}
    </label>
  );
}

function SensitiveConfigField({
  label,
  value,
  onChange,
  placeholder,
  helper,
  compact = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  helper?: string;
  compact?: boolean;
}) {
  const inputId = useId();
  const [revealed, setRevealed] = useState(false);
  const revealLabel = revealed ? "Hide value" : "Reveal value";

  return (
    <div className="block min-w-0">
      <label htmlFor={inputId} className="mb-1 block text-sm font-medium">
        {label}
      </label>
      <div className="relative min-w-0">
        <Input
          id={inputId}
          aria-label={label}
          type={revealed ? "text" : "password"}
          size={compact ? "sm" : "base"}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
          spellCheck={false}
          className={cn("w-full min-w-0", compact ? "pr-12" : "pr-14")}
        />
        <button
          type="button"
          aria-label={revealLabel}
          aria-pressed={revealed}
          title={revealLabel}
          onClick={() => setRevealed((current) => !current)}
          className={cn(
            "absolute right-0 top-1/2 z-10 flex size-11 -translate-y-1/2 items-center justify-center rounded-md text-kumo-subtle transition-colors hover:bg-kumo-tint hover:text-kumo-default focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand",
            compact ? "text-xs" : "text-sm",
          )}
        >
          {revealed ? (
            <EyeSlash size={16} aria-hidden="true" />
          ) : (
            <Eye size={16} aria-hidden="true" />
          )}
        </button>
      </div>
      {!compact && helper && (
        <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
          {helper}
        </span>
      )}
    </div>
  );
}

function ConfigSummary({
  title,
  config,
  routes,
  emptyLabel,
}: {
  title: string;
  config: WebClientConfig;
  routes?: WebSocketRoutes;
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
          value={valueOrEmpty(routes?.id ?? "", emptyLabel)}
        />
        <SummaryRow
          label={t("wsRelayHost")}
          value={valueOrEmpty(routes?.relay ?? "", emptyLabel)}
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

function RuntimeSummary({
  localActive,
  serverLoading,
  serverError,
}: {
  localActive: boolean;
  serverLoading: boolean;
  serverError: boolean;
}) {
  const { t } = useTranslation();
  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
      <div className="flex items-start gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
          <GlobeHemisphereWest size={18} />
        </div>
        <div>
          <h2 className="text-base font-semibold">{t("webClientRuntime")}</h2>
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
            serverError
              ? t("operationFailed")
              : serverLoading
                ? t("loading")
                : t("loaded")
          }
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
  };
  const selectedHint =
    mode === "auto"
      ? t("webSocketModeAutoHint")
      : mode === "proxy"
        ? t("webSocketModeProxyHint")
        : t("webSocketModeCustomHint");
  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-sm font-medium">{t("webSocketMode")}</span>
        <CompactButtonGroup<WsMode>
          ariaLabel={t("webSocketMode")}
          value={mode}
          onChange={changeMode}
          options={[
            { value: "auto", label: t("webSocketModeAuto") },
            { value: "proxy", label: t("webSocketModeProxy") },
            { value: "custom", label: t("webSocketModeCustom") },
          ]}
        />
        <span className="min-w-0 text-sm leading-6 text-kumo-subtle">
          {selectedHint}
        </span>
      </div>

      {mode === "proxy" && (
        <div className="max-w-3xl">
          <ConfigField
            label={t("wsHost")}
            storageKey={STORAGE_KEYS.ws_host}
            value={config.ws_host}
            onChange={(value) => updateField("ws_host", value)}
            placeholder="https://rd.example.com"
            helper={t("wsHostHint")}
          />
        </div>
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
    <div className="block min-w-0">
      <span className="mb-1 block text-sm font-medium text-kumo-subtle">
        {label}
      </span>
      <div className="min-h-9 break-all rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 font-mono text-sm leading-5 text-kumo-default">
        {value}
      </div>
    </div>
  );
}

function DeploymentPreview({
  deployment,
  loading,
  error,
  platform,
  setPlatform,
  deploymentPassword,
  setDeploymentPassword,
  approveMode,
  setApproveMode,
  verificationMethod,
  setVerificationMethod,
  useUnattendedDefaults,
  clearDeploymentOptions,
}: {
  deployment?: DeploymentConfig;
  loading: boolean;
  error?: Error | null;
  platform: DeploymentPlatform;
  setPlatform: (platform: DeploymentPlatform) => void;
  deploymentPassword: string;
  setDeploymentPassword: (value: string) => void;
  approveMode: ApproveMode;
  setApproveMode: (value: ApproveMode) => void;
  verificationMethod: VerificationMethod;
  setVerificationMethod: (value: VerificationMethod) => void;
  useUnattendedDefaults: () => void;
  clearDeploymentOptions: () => void;
}) {
  const { t } = useTranslation();
  const hasDeploymentOptions =
    Boolean(deploymentPassword.trim()) ||
    Boolean(approveMode) ||
    Boolean(verificationMethod);
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
      <DeploymentAccessOptions
        deploymentPassword={deploymentPassword}
        setDeploymentPassword={setDeploymentPassword}
        approveMode={approveMode}
        setApproveMode={setApproveMode}
        verificationMethod={verificationMethod}
        setVerificationMethod={setVerificationMethod}
        useUnattendedDefaults={useUnattendedDefaults}
        clearDeploymentOptions={clearDeploymentOptions}
        hasDeploymentOptions={hasDeploymentOptions}
      />
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
        <div className="mt-4 grid gap-4">
          <DeploymentUsage />
          <div className="grid gap-3 lg:grid-cols-2">
            <CopyField
              label={t("idWebSocket")}
              description={t("idWebSocketDeploymentHint")}
              value={deployment.webclient_ws_routes.id}
            />
            <CopyField
              label={t("relayWebSocket")}
              description={t("relayWebSocketDeploymentHint")}
              value={deployment.webclient_ws_routes.relay}
            />
          </div>
          <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]">
            <CopyField
              label={t("encodedConfig")}
              description={t("encodedConfigHint")}
              value={deployment.encoded_config}
            />
            <CopyField
              label={t("filenameHint")}
              description={t("filenameHintHint")}
              value={deployment.filename_hint}
            />
          </div>
          <MobileConfigPanel
            value={deployment.mobile_config_text}
            uri={deployment.mobile_config_uri}
          />
          <div className="rounded-md border border-kumo-line bg-kumo-base p-3">
            <div className="flex flex-col items-start gap-3">
              <div>
                <h3 className="text-sm font-semibold">
                  {t("deploymentCommands")}
                </h3>
                <p className="mt-1 text-xs leading-5 text-kumo-subtle">
                  {t("deploymentCommandsHint")}
                </p>
              </div>
              <Tabs
                variant="segmented"
                size="sm"
                value={platform}
                onValueChange={(value) =>
                  setPlatform(value as DeploymentPlatform)
                }
                tabs={[
                  { value: "linux", label: t("linux") },
                  { value: "macos", label: t("macos") },
                  { value: "windows", label: t("windows") },
                ]}
              />
            </div>
            <div className="mt-3 grid gap-3 lg:grid-cols-2">
              <CommandBlock
                title={t("configCommand")}
                description={t("configCommandHint")}
                commands={deployment.config_command[platform]}
              />
              <CommandBlock
                title={t("optionCommands")}
                description={t("optionCommandsHint")}
                commands={deployment.option_commands[platform]}
              />
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function MobileConfigPanel({ value, uri }: { value: string; uri: string }) {
  const { t } = useTranslation();
  const [qrDataUrl, setQrDataUrl] = useState("");
  const [qrError, setQrError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setQrDataUrl("");
    setQrError("");
    if (!value) return;

    QRCode.toDataURL(value, {
      errorCorrectionLevel: "M",
      margin: 2,
      width: 192,
    })
      .then((url) => {
        if (!cancelled) setQrDataUrl(url);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setQrError(error instanceof Error ? error.message : t("operationFailed"));
        }
      });

    return () => {
      cancelled = true;
    };
  }, [t, value]);

  return (
    <div className="rounded-md border border-kumo-line bg-kumo-base p-3">
      <div className="flex items-start gap-3">
        <div className="flex size-8 shrink-0 items-center justify-center rounded-md border border-kumo-line bg-kumo-elevated text-kumo-brand">
          <QrCode size={17} />
        </div>
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">{t("mobileDevices")}</h3>
          <p className="mt-1 text-xs leading-5 text-kumo-subtle">
            {t("mobileDevicesHint")}
          </p>
        </div>
      </div>

      <div className="mt-3 grid gap-3 lg:grid-cols-[220px_minmax(0,1fr)]">
        <div className="flex justify-center rounded-lg border border-kumo-line bg-white p-3">
          {qrDataUrl ? (
            <img
              src={qrDataUrl}
              alt={t("mobileConfigQrAlt")}
              className="size-48"
            />
          ) : (
            <div
              role="status"
              className="flex size-48 items-center justify-center rounded-md border border-kumo-line bg-kumo-base p-4 text-center text-xs leading-5 text-kumo-subtle"
            >
              {qrError || t("loading")}
            </div>
          )}
        </div>
        <div className="grid min-w-0 content-start gap-3">
          <p className="rounded-md border border-kumo-line bg-kumo-elevated px-3 py-2 text-xs leading-5 text-kumo-subtle">
            {t("mobileConfigSecurityHint")}
          </p>
          <CopyField
            label={t("mobileConfigText")}
            description={t("mobileConfigTextHint")}
            value={value}
          />
          <CopyField
            label={t("mobileConfigUri")}
            description={t("mobileConfigUriHint")}
            value={uri}
          />
        </div>
      </div>
    </div>
  );
}

function DeploymentAccessOptions({
  deploymentPassword,
  setDeploymentPassword,
  approveMode,
  setApproveMode,
  verificationMethod,
  setVerificationMethod,
  useUnattendedDefaults,
  clearDeploymentOptions,
  hasDeploymentOptions,
}: {
  deploymentPassword: string;
  setDeploymentPassword: (value: string) => void;
  approveMode: ApproveMode;
  setApproveMode: (value: ApproveMode) => void;
  verificationMethod: VerificationMethod;
  setVerificationMethod: (value: VerificationMethod) => void;
  useUnattendedDefaults: () => void;
  clearDeploymentOptions: () => void;
  hasDeploymentOptions: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="mt-4 rounded-md border border-kumo-line bg-kumo-base p-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold">{t("deploymentAccessOptions")}</h3>
          <p className="mt-1 text-xs leading-5 text-kumo-subtle">
            {t("deploymentAccessOptionsHint")}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={useUnattendedDefaults}
          >
            {t("deploymentUseUnattendedDefaults")}
          </Button>
          {hasDeploymentOptions && (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={clearDeploymentOptions}
            >
              {t("deploymentClearOptional")}
            </Button>
          )}
        </div>
      </div>

      <div className="mt-3 grid gap-3">
        <label className="block min-w-0">
          <span className="mb-1 block text-sm font-medium">
            {t("deploymentPermanentPassword")}
          </span>
          <Input
            aria-label={t("deploymentPermanentPassword")}
            type="password"
            value={deploymentPassword}
            onChange={(event) => setDeploymentPassword(event.target.value)}
            placeholder={t("deploymentPermanentPasswordPlaceholder")}
            autoComplete="new-password"
            spellCheck={false}
            size="sm"
            className="w-full min-w-0 md:max-w-xl"
          />
          <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
            {t("deploymentPermanentPasswordHint")}
          </span>
        </label>

        <div className="grid gap-3 xl:grid-cols-2">
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm font-medium">{t("deploymentApproveMode")}</span>
            <CompactButtonGroup<ApproveMode>
              ariaLabel={t("deploymentApproveMode")}
              value={approveMode}
              onChange={setApproveMode}
              options={[
                { value: "", label: t("doNotChange") },
                { value: "password", label: t("approveModePassword") },
                { value: "click", label: t("approveModeClick") },
                {
                  value: "password-click",
                  label: t("approveModePasswordClick"),
                },
              ]}
            />
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm font-medium">
              {t("deploymentVerificationMethod")}
            </span>
            <CompactButtonGroup<VerificationMethod>
              ariaLabel={t("deploymentVerificationMethod")}
              value={verificationMethod}
              onChange={setVerificationMethod}
              options={[
                { value: "", label: t("doNotChange") },
                {
                  value: "use-permanent-password",
                  label: t("verificationPermanentOnly"),
                },
                {
                  value: "use-temporary-password",
                  label: t("verificationTemporaryOnly"),
                },
                {
                  value: "use-both-passwords",
                  label: t("verificationBothPasswords"),
                },
              ]}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

function RecordStoragePanel({
  config,
  loading,
  error,
  onChange,
  saving,
  saveDisabled,
  onSave,
}: {
  config: RecordStorageConfig;
  loading: boolean;
  error?: Error | null;
  onChange: (next: RecordStorageConfig) => void;
  saving: boolean;
  saveDisabled: boolean;
  onSave: () => void;
}) {
  const { t } = useTranslation();
  const setType = (type: RecordStorageType) => onChange({ ...config, type });
  const setRoot = (
    field: "local_dir" | "temp_dir",
    value: string,
  ) => onChange({ ...config, [field]: value });
  const setS3 = <K extends keyof RecordStorageConfig["s3"]>(
    field: K,
    value: RecordStorageConfig["s3"][K],
  ) => onChange({ ...config, s3: { ...config.s3, [field]: value } });
  const setWebDav = <K extends keyof RecordStorageConfig["webdav"]>(
    field: K,
    value: RecordStorageConfig["webdav"][K],
  ) => onChange({ ...config, webdav: { ...config.webdav, [field]: value } });
  const remote = config.type === "s3" || config.type === "webdav";
  const storageHint =
    config.type === "local"
      ? t("recordStorageLocalHint")
      : config.type === "s3"
        ? t("recordStorageS3Hint")
        : t("recordStorageWebDavHint");

  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated">
      <div className="flex items-start gap-3 p-5">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
          <CloudArrowUp size={18} />
        </div>
        <div>
          <h2 className="text-base font-semibold">{t("recordStorage")}</h2>
          <p className="mt-1 text-sm leading-6 text-kumo-subtle">
            {t("recordStorageHint")}
          </p>
        </div>
      </div>

      {loading && (
        <p
          role="status"
          className="mx-5 rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm text-kumo-subtle"
        >
          {t("loading")}
        </p>
      )}
      {error && (
        <InlineMessage tone="error" className="mx-5">
          {error.message || t("operationFailed")}
        </InlineMessage>
      )}

      <div className="grid gap-5 px-5 pb-5">
        <div className="flex flex-wrap items-center gap-3">
          <span className="text-sm font-medium">{t("recordStorageBackend")}</span>
          <CompactButtonGroup<RecordStorageType>
            ariaLabel={t("recordStorageBackend")}
            value={config.type}
            onChange={setType}
            options={[
              { value: "local", label: t("recordStorageLocal") },
              { value: "s3", label: t("recordStorageS3") },
              { value: "webdav", label: t("recordStorageWebDav") },
            ]}
          />
          <span className="min-w-0 text-sm leading-6 text-kumo-subtle">
            {storageHint}
          </span>
        </div>

        <div className="grid gap-4 lg:grid-cols-2">
          {config.type === "local" && (
            <ConfigField
              label={t("recordStorageLocalDir")}
              storageKey="RUSTDESK_API_RECORD_STORAGE_LOCAL_DIR"
              value={config.local_dir}
              onChange={(value) => setRoot("local_dir", value)}
              placeholder="resources/record"
              helper={t("recordStorageLocalDirHint")}
            />
          )}
          {remote && (
            <ConfigField
              label={t("recordStorageTempDir")}
              storageKey="RUSTDESK_API_RECORD_STORAGE_TEMP_DIR"
              value={config.temp_dir}
              onChange={(value) => setRoot("temp_dir", value)}
              placeholder="resources/record/tmp"
              helper={t("recordStorageTempDirHint")}
            />
          )}
        </div>

        {config.type === "s3" && (
          <div className="grid gap-4">
            <div className="grid gap-4 lg:grid-cols-2">
              <ConfigField
                label={t("s3Endpoint")}
                storageKey="RUSTDESK_API_RECORD_STORAGE_S3_ENDPOINT"
                value={config.s3.endpoint}
                onChange={(value) => setS3("endpoint", value)}
                placeholder="https://s3.example.com"
              />
              <ConfigField
                label={t("s3Region")}
                storageKey="RUSTDESK_API_RECORD_STORAGE_S3_REGION"
                value={config.s3.region}
                onChange={(value) => setS3("region", value)}
                placeholder="auto"
              />
              <ConfigField
                label={t("s3Bucket")}
                storageKey="RUSTDESK_API_RECORD_STORAGE_S3_BUCKET"
                value={config.s3.bucket}
                onChange={(value) => setS3("bucket", value)}
                placeholder="rustdesk-recordings"
              />
              <ConfigField
                label={t("storagePrefix")}
                storageKey="RUSTDESK_API_RECORD_STORAGE_S3_PREFIX"
                value={config.s3.prefix}
                onChange={(value) => setS3("prefix", value)}
                placeholder="record/"
              />
              <SecretConfigField
                label={t("s3AccessKeyId")}
                storageKey="RUSTDESK_API_RECORD_STORAGE_S3_ACCESS_KEY_ID"
                value={config.s3.access_key_id}
                configured={config.s3.access_key_id_configured}
                clear={Boolean(config.s3.clear_access_key_id)}
                onChange={(value) => setS3("access_key_id", value)}
                onClearChange={(value) => setS3("clear_access_key_id", value)}
              />
              <SecretConfigField
                label={t("s3SecretAccessKey")}
                storageKey="RUSTDESK_API_RECORD_STORAGE_S3_SECRET_ACCESS_KEY"
                value={config.s3.secret_access_key}
                configured={config.s3.secret_access_key_configured}
                clear={Boolean(config.s3.clear_secret_access_key)}
                onChange={(value) => setS3("secret_access_key", value)}
                onClearChange={(value) =>
                  setS3("clear_secret_access_key", value)
                }
              />
            </div>
            <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-2">
              <Switch
                label={t("s3ForcePathStyle")}
                labelTooltip={t("s3ForcePathStyleHint")}
                controlFirst={false}
                checked={config.s3.force_path_style}
                onCheckedChange={(checked: boolean) =>
                  setS3("force_path_style", checked)
                }
              />
            </div>
          </div>
        )}

        {config.type === "webdav" && (
          <div className="grid gap-4 lg:grid-cols-2">
            <ConfigField
              label={t("webDavUrl")}
              storageKey="RUSTDESK_API_RECORD_STORAGE_WEBDAV_URL"
              value={config.webdav.url}
              onChange={(value) => setWebDav("url", value)}
              placeholder="https://dav.example.com/remote.php/dav/files/user"
            />
            <ConfigField
              label={t("storagePrefix")}
              storageKey="RUSTDESK_API_RECORD_STORAGE_WEBDAV_PREFIX"
              value={config.webdav.prefix}
              onChange={(value) => setWebDav("prefix", value)}
              placeholder="record/"
            />
            <ConfigField
              label={t("webDavUsername")}
              storageKey="RUSTDESK_API_RECORD_STORAGE_WEBDAV_USERNAME"
              value={config.webdav.username}
              onChange={(value) => setWebDav("username", value)}
              placeholder="rustdesk"
            />
            <SecretConfigField
              label={t("webDavPassword")}
              storageKey="RUSTDESK_API_RECORD_STORAGE_WEBDAV_PASSWORD"
              value={config.webdav.password}
              configured={config.webdav.password_configured}
              clear={Boolean(config.webdav.clear_password)}
              onChange={(value) => setWebDav("password", value)}
              onClearChange={(value) => setWebDav("clear_password", value)}
            />
          </div>
        )}

        <p className="rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm leading-6 text-kumo-subtle">
          {t("recordStorageApplyHint")}
        </p>
      </div>
      <div className="flex justify-end border-t border-kumo-line bg-kumo-recessed px-5 py-4">
        <Button
          type="button"
          size="sm"
          onClick={onSave}
          disabled={saveDisabled}
          loading={saving}
        >
          {t("writeRecordStorageConfig")}
        </Button>
      </div>
    </section>
  );
}

function SecretConfigField({
  label,
  storageKey,
  value,
  configured,
  clear,
  onChange,
  onClearChange,
}: {
  label: string;
  storageKey: string;
  value: string;
  configured: boolean;
  clear: boolean;
  onChange: (value: string) => void;
  onClearChange: (value: boolean) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-2">
      <label className="block min-w-0">
        <span className="mb-1 block text-sm font-medium">{label}</span>
        <Input
          aria-label={label}
          type="password"
          value={value}
          disabled={clear}
          onChange={(e) => onChange(e.target.value)}
          placeholder={
            configured
              ? t("secretConfiguredPlaceholder")
              : t("secretEmptyPlaceholder")
          }
          spellCheck={false}
          className="w-full min-w-0"
        />
        <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
          {storageKey}
        </span>
      </label>
      {configured && (
        <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-2">
          <Switch
            label={t("clearConfiguredSecret")}
            controlFirst={false}
            checked={clear}
            onCheckedChange={(checked: boolean) => onClearChange(checked)}
          />
        </div>
      )}
    </div>
  );
}

function DeploymentUsage() {
  const { t } = useTranslation();
  const steps = [
    t("deploymentUsageStepSave"),
    t("deploymentUsageStepPlatform"),
    t("deploymentUsageStepChoose"),
  ];
  return (
    <div className="rounded-md border border-kumo-line bg-kumo-base p-3">
      <h3 className="text-sm font-semibold">{t("deploymentUsage")}</h3>
      <ol className="mt-3 grid gap-2 md:grid-cols-3">
        {steps.map((step, index) => (
          <li
            key={step}
            className="grid grid-cols-[1.5rem_minmax(0,1fr)] gap-2 text-sm leading-6"
          >
            <span className="flex size-6 items-center justify-center rounded border border-kumo-line bg-kumo-elevated font-mono text-xs">
              {index + 1}
            </span>
            <span className="text-kumo-subtle">{step}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}

function CommandBlock({
  title,
  description,
  commands,
}: {
  title: string;
  description: string;
  commands: string[];
}) {
  return (
    <div className="grid gap-2">
      <div>
        <p className="text-xs font-medium text-kumo-subtle">{title}</p>
        <p className="mt-1 text-xs leading-5 text-kumo-subtle">
          {description}
        </p>
      </div>
      <CopyField label={title} value={commands.join("\n")} />
    </div>
  );
}

function CopyField({
  label,
  value,
  description,
}: {
  label: string;
  value: string;
  description?: string;
}) {
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
        <div className="min-w-0">
          <span className="block break-all text-xs font-medium text-kumo-subtle">
            {label}
          </span>
          {description && (
            <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
              {description}
            </span>
          )}
        </div>
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
