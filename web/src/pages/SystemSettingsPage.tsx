import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import {
  CheckCircle,
  EnvelopeSimple,
  Eye,
  GearSix,
  Info,
  NotePencil,
  PaperPlaneTilt,
  Plus,
  ShieldCheck,
  Trash,
} from "@phosphor-icons/react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPatch, apiPost, ApiError } from "../lib/api";

interface AdminConfigView {
  title: string;
  hello: string;
  hello_raw: string;
  hello_file: string;
  timezone?: string;
}

interface AdminConfigForm {
  title: string;
  hello: string;
  hello_file: string;
}
interface LoginSecuritySettings {
  require_totp: boolean;
  require_email_verification: boolean;
  require_device_verification: boolean;
  allow_trusted_login_devices: boolean;
}
interface EmailSettingsView {
  id: number;
  name: string;
  host: string;
  port: number;
  username: string;
  from: string;
  tls: string;
  enabled: boolean;
  password_set: boolean;
  configured: boolean;
}
interface LoginSecurityConfigView {
  login: LoginSecuritySettings;
}
interface EmailSettingsForm {
  id: number;
  name: string;
  host: string;
  port: number;
  username: string;
  password: string;
  clear_password: boolean;
  from: string;
  tls: string;
}
type SettingsTab = "site" | "security";

const emptyForm: AdminConfigForm = {
  title: "",
  hello: "",
  hello_file: "",
};
const emptyLoginSecurity: LoginSecuritySettings = {
  require_totp: false,
  require_email_verification: false,
  require_device_verification: false,
  allow_trusted_login_devices: true,
};
const emptyEmailForm: EmailSettingsForm = {
  id: 0,
  name: "",
  host: "",
  port: 587,
  username: "",
  password: "",
  clear_password: false,
  from: "",
  tls: "starttls",
};

function normalizeConfig(config: Partial<AdminConfigView>): AdminConfigForm {
  return {
    title: config.title ?? "",
    hello: config.hello_raw ?? "",
    hello_file: config.hello_file ?? "",
  };
}

function normalizeSecurityConfig(
  config: Partial<LoginSecurityConfigView>,
): LoginSecuritySettings {
  return {
    ...emptyLoginSecurity,
    ...(config.login ?? {}),
  };
}

function normalizeEmailConfig(config?: Partial<EmailSettingsView>): EmailSettingsForm {
  return {
    ...emptyEmailForm,
    ...(config
      ? {
          id: config.id ?? 0,
          name: config.name ?? "",
          host: config.host ?? "",
          port: config.port || 587,
          username: config.username ?? "",
          from: config.from ?? "",
          tls: config.tls || "starttls",
        }
      : {}),
    password: "",
    clear_password: false,
  };
}

export function SystemSettingsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [form, setForm] = useState<AdminConfigForm>(emptyForm);
  const [loginForm, setLoginForm] =
    useState<LoginSecuritySettings>(emptyLoginSecurity);
  const [emailForm, setEmailForm] = useState<EmailSettingsForm>(emptyEmailForm);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [securityMessage, setSecurityMessage] = useState("");
  const [securityError, setSecurityError] = useState("");
  const [testEmail, setTestEmail] = useState("");
  const [activeTab, setActiveTab] = useState<SettingsTab>("site");
  const [previewOpen, setPreviewOpen] = useState(false);
  const [selectedSmtpId, setSelectedSmtpId] = useState<number | null>(null);
  const [smtpCreating, setSmtpCreating] = useState(false);
  const [smtpDeleteTarget, setSmtpDeleteTarget] =
    useState<EmailSettingsView | null>(null);

  const config = useQuery({
    queryKey: ["admin-panel-config"],
    queryFn: () => apiGet<AdminConfigView>("/api/admin/config/admin/manage"),
    staleTime: 0,
    refetchOnMount: "always",
  });
  const loginSecurity = useQuery({
    queryKey: ["login-security-config"],
    queryFn: () =>
      apiGet<LoginSecurityConfigView>("/api/admin/config/login-security"),
    staleTime: 0,
    refetchOnMount: "always",
  });
  const smtpConfigs = useQuery({
    queryKey: ["smtp-email-configs"],
    queryFn: () => apiGet<EmailSettingsView[]>("/api/admin/config/smtp"),
    staleTime: 0,
    refetchOnMount: "always",
  });

  useEffect(() => {
    if (config.data) setForm(normalizeConfig(config.data));
  }, [config.data]);
  useEffect(() => {
    if (loginSecurity.data) {
      setLoginForm(normalizeSecurityConfig(loginSecurity.data));
    }
  }, [loginSecurity.data]);
  useEffect(() => {
    if (!smtpConfigs.data) return;
    if (smtpCreating) {
      return;
    }
    if (selectedSmtpId !== null && selectedSmtpId > 0) {
      const selected = smtpConfigs.data.find((row) => row.id === selectedSmtpId);
      if (selected) {
        setEmailForm(normalizeEmailConfig(selected));
      }
      return;
    }
    const selected = smtpConfigs.data.find((row) => row.enabled) ?? smtpConfigs.data[0];
    if (selected) {
      setSelectedSmtpId(selected.id);
      setEmailForm(normalizeEmailConfig(selected));
    } else {
      setSelectedSmtpId(0);
      setEmailForm(emptyEmailForm);
    }
  }, [selectedSmtpId, smtpConfigs.data, smtpCreating]);

  const save = useMutation({
    mutationFn: (payload: AdminConfigForm) =>
      apiPatch<AdminConfigView>("/api/admin/config/admin/manage", payload),
    onSuccess: (saved) => {
      setForm(normalizeConfig(saved));
      setMessage(t("siteWelcomeSaved"));
      setError("");
      void qc.invalidateQueries({ queryKey: ["admin-panel-config"] });
      void qc.invalidateQueries({ queryKey: ["public-admin-config"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
      setMessage("");
    },
  });
  const saveSecurity = useMutation({
    mutationFn: (payload: { login: LoginSecuritySettings }) =>
      apiPatch<LoginSecurityConfigView>("/api/admin/config/login-security", payload),
    onSuccess: (saved) => {
      setLoginForm(normalizeSecurityConfig(saved));
      setSecurityMessage(t("loginSecuritySaved"));
      setSecurityError("");
      void qc.invalidateQueries({ queryKey: ["login-security-config"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
      setSecurityMessage("");
    },
  });
  const saveSmtp = useMutation({
    mutationFn: (payload: EmailSettingsForm) =>
      apiPost<EmailSettingsView>("/api/admin/config/smtp", {
        id: payload.id,
        name: payload.name.trim(),
        host: payload.host.trim(),
        port: payload.port,
        username: payload.username.trim(),
        password: payload.password,
        clear_password: payload.clear_password,
        from: payload.from.trim(),
        tls: payload.tls,
      }),
    onSuccess: (saved) => {
      setSmtpCreating(false);
      setSelectedSmtpId(saved.id);
      setEmailForm(normalizeEmailConfig(saved));
      setSecurityMessage(t("smtpConfigSaved"));
      setSecurityError("");
      void qc.invalidateQueries({ queryKey: ["smtp-email-configs"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
      setSecurityMessage("");
    },
  });
  const enableSmtp = useMutation({
    mutationFn: (id: number) =>
      apiPost<EmailSettingsView>("/api/admin/config/smtp/enable", { id }),
    onSuccess: (saved) => {
      setSmtpCreating(false);
      setSelectedSmtpId(saved.id);
      setEmailForm(normalizeEmailConfig(saved));
      setSecurityMessage(t("smtpConfigEnabled"));
      setSecurityError("");
      void qc.invalidateQueries({ queryKey: ["smtp-email-configs"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
      setSecurityMessage("");
    },
  });
  const deleteSmtp = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/config/smtp/delete", { id }),
    onSuccess: () => {
      setSmtpDeleteTarget(null);
      setSmtpCreating(false);
      setSelectedSmtpId(0);
      setEmailForm(emptyEmailForm);
      setSecurityMessage(t("smtpConfigDeleted"));
      setSecurityError("");
      void qc.invalidateQueries({ queryKey: ["smtp-email-configs"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
      setSecurityMessage("");
    },
  });
  const testEmailMutation = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/config/smtp/test", {
        id: emailForm.id,
        to: testEmail.trim(),
      }),
    onSuccess: () => {
      setSecurityMessage(t("testEmailSent"));
      setSecurityError("");
    },
    onError: (err) => {
      const ae = err as ApiError;
      setSecurityError(ae.message || t("operationFailed"));
      setSecurityMessage("");
    },
  });

  const updateField = (field: keyof AdminConfigForm, value: string) => {
    setForm((current) => ({ ...current, [field]: value }));
    setMessage("");
    setError("");
  };
  const updateLoginSecurity = (
    field: keyof LoginSecuritySettings,
    value: boolean,
  ) => {
    setLoginForm((current) => ({
      ...current,
      [field]: value,
    }));
    setSecurityMessage("");
    setSecurityError("");
  };
  const updateEmail = <K extends keyof EmailSettingsForm>(
    field: K,
    value: EmailSettingsForm[K],
  ) => {
    setEmailForm((current) => ({
      ...current,
      [field]: value,
    }));
    setSecurityMessage("");
    setSecurityError("");
  };

  const inlineHello = form.hello.trim();
  const filePath = form.hello_file.trim();
  const canReuseFilePreview =
    filePath &&
    filePath === config.data?.hello_file?.trim() &&
    !config.data?.hello_raw?.trim() &&
    config.data?.hello;
  const preview = inlineHello
    ? form.hello.split("{{username}}").join(t("username"))
    : canReuseFilePreview
      ? (config.data?.hello ?? "")
      : filePath
        ? t("siteWelcomeFilePreviewPending")
        : "";
  const enabledSmtpConfig = smtpConfigs.data?.find((row) => row.enabled);
  const selectedSmtpConfig = smtpConfigs.data?.find((row) => row.id === emailForm.id);
  const emailConfigured = Boolean(enabledSmtpConfig?.configured);
  const emailRequired =
    loginForm.require_email_verification ||
    loginForm.require_device_verification;
  const saveSiteSettings = () =>
    save.mutate({
      title: form.title.trim(),
      hello: form.hello.trim(),
      hello_file: form.hello_file.trim(),
    });
  const saveLoginSecurity = () =>
    saveSecurity.mutate({
      login: loginForm,
    });
  const saveSmtpConfig = () => saveSmtp.mutate(emailForm);
  const startNewSmtpConfig = () => {
    setSmtpCreating(true);
    setSelectedSmtpId(0);
    setEmailForm(emptyEmailForm);
    setSecurityMessage("");
    setSecurityError("");
  };
  const selectSmtpConfig = (config: EmailSettingsView) => {
    setSmtpCreating(false);
    setSelectedSmtpId(config.id);
    setEmailForm(normalizeEmailConfig(config));
    setSecurityMessage("");
    setSecurityError("");
  };
  const tabs: Array<{ key: SettingsTab; label: string; hint: string }> = [
    {
      key: "site",
      label: t("settingsTabSiteIdentity"),
      hint: t("settingsTabSiteIdentityHint"),
    },
    {
      key: "security",
      label: t("settingsTabLoginSecurity"),
      hint: t("settingsTabLoginSecurityHint"),
    },
  ];

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("systemSettings")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("systemSettingsHint")}
          </p>
        </div>
        {activeTab === "site" && (
          <Button loading={save.isPending} onClick={saveSiteSettings}>
            <GearSix size={16} />
            {t("writeSystemSettings")}
          </Button>
        )}
      </div>

      <div
        role="tablist"
        aria-label={t("systemSettings")}
        className="grid gap-1 rounded-lg border border-kumo-line bg-kumo-elevated p-1 sm:grid-cols-2"
      >
        {tabs.map((tab) => {
          const selected = activeTab === tab.key;
          return (
            <button
              key={tab.key}
              type="button"
              role="tab"
              aria-selected={selected}
              className={[
                "min-h-11 rounded-md px-3 py-2 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand",
                selected
                  ? "bg-kumo-base text-kumo-default shadow-sm"
                  : "text-kumo-subtle hover:bg-kumo-tint/60 hover:text-kumo-default",
              ].join(" ")}
              onClick={() => setActiveTab(tab.key)}
            >
              <span className="block text-sm font-semibold">{tab.label}</span>
              <span className="mt-0.5 block text-xs leading-5">{tab.hint}</span>
            </button>
          );
        })}
      </div>

      {activeTab === "site" && message && (
        <InlineMessage tone="success">{message}</InlineMessage>
      )}
      {activeTab === "site" && error && (
        <InlineMessage tone="error">{error}</InlineMessage>
      )}
      {activeTab === "security" && securityMessage && (
        <InlineMessage tone="success">{securityMessage}</InlineMessage>
      )}
      {activeTab === "security" && securityError && (
        <InlineMessage tone="error">{securityError}</InlineMessage>
      )}

      {activeTab === "site" && (
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="mb-5 flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
            <div className="flex items-start gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <NotePencil size={18} />
              </div>
              <div>
                <h2 className="text-base font-semibold">{t("siteIdentity")}</h2>
                <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
                  {t("siteIdentityHint")}
                </p>
              </div>
            </div>
            <Button
              type="button"
              variant="secondary"
              className="w-full sm:w-auto"
              onClick={() => setPreviewOpen(true)}
            >
              <Eye size={16} />
              {t("previewWelcomeMessage")}
            </Button>
          </div>

          {config.isLoading && <TableState tone="loading">{t("loading")}</TableState>}
          {config.error && (
            <TableState tone="error">
              {(config.error as Error).message || t("operationFailed")}
            </TableState>
          )}

          {!config.isLoading && !config.error && (
            <div className="grid gap-5">
              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("siteTitle")}
                </span>
                <Input
                  aria-label={t("siteTitle")}
                  value={form.title}
                  maxLength={80}
                  onChange={(e) => updateField("title", e.target.value)}
                />
                <span className="mt-1.5 block text-xs text-kumo-subtle">
                  {t("siteTitleHint")}
                </span>
              </label>

              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("siteWelcomeFile")}
                </span>
                <Input
                  aria-label={t("siteWelcomeFile")}
                  value={form.hello_file}
                  maxLength={500}
                  placeholder="/etc/rustdesk-console/welcome.txt"
                  onChange={(e) => updateField("hello_file", e.target.value)}
                />
                <span className="mt-1.5 block text-xs text-kumo-subtle">
                  {t("siteWelcomeFileHint")}
                </span>
              </label>

              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("siteWelcomeInline")}
                </span>
                <textarea
                  className="min-h-40 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm leading-6 focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                  aria-label={t("siteWelcomeInline")}
                  value={form.hello}
                  maxLength={5000}
                  onChange={(e) => updateField("hello", e.target.value)}
                />
                <span className="mt-1.5 block text-xs text-kumo-subtle">
                  {t("siteWelcomeInlineHint")}
                </span>
              </label>
              <div className="flex items-start gap-3 rounded-lg border border-kumo-line bg-kumo-base px-3 py-3 text-sm leading-6 text-kumo-subtle">
                <Info className="mt-0.5 shrink-0 text-kumo-brand" size={16} />
                <p>{t("siteWelcomeFilePriority")}</p>
              </div>
            </div>
          )}
        </section>
      )}

      {activeTab === "security" && (
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="mb-5 flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
            <div className="flex items-start gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <ShieldCheck size={18} />
              </div>
              <div>
                <h2 className="text-base font-semibold">{t("loginSecurity")}</h2>
                <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
                  {t("loginSecurityHint")}
                </p>
              </div>
            </div>
          </div>

          {emailRequired && !emailConfigured && (
            <p className="mb-4 rounded-md border border-kumo-danger/25 bg-kumo-danger-tint/30 px-3 py-2 text-sm leading-6 text-kumo-danger">
              {t("loginSecurityEmailRequiredHint")}
            </p>
          )}

          {(loginSecurity.isLoading || smtpConfigs.isLoading) && (
            <TableState tone="loading">{t("loading")}</TableState>
          )}
          {(loginSecurity.error || smtpConfigs.error) && (
            <TableState tone="error">
              {((loginSecurity.error || smtpConfigs.error) as Error).message ||
                t("operationFailed")}
            </TableState>
          )}

          {!loginSecurity.isLoading &&
            !smtpConfigs.isLoading &&
            !loginSecurity.error &&
            !smtpConfigs.error && (
            <div className="grid gap-5">
              <section className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                <div className="mb-4 flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                  <div>
                    <h3 className="text-sm font-semibold">
                      {t("loginSecurityPolicy")}
                    </h3>
                    <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                      {t("loginSecurityPolicyHint")}
                    </p>
                  </div>
                  <Button
                    type="button"
                    className="w-full md:w-auto"
                    loading={saveSecurity.isPending}
                    disabled={saveSecurity.isPending}
                    onClick={saveLoginSecurity}
                  >
                    <ShieldCheck size={16} />
                    {t("saveLoginSecurity")}
                  </Button>
                </div>
                <div className="grid gap-3 md:grid-cols-2">
                  <SecuritySwitch
                    title={t("requireTotp")}
                    description={t("requireTotpHint")}
                    checked={loginForm.require_totp}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("require_totp", value)
                    }
                  />
                  <SecuritySwitch
                    title={t("requireEmailVerification")}
                    description={t("requireEmailVerificationHint")}
                    checked={loginForm.require_email_verification}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("require_email_verification", value)
                    }
                  />
                  <SecuritySwitch
                    title={t("requireDeviceVerification")}
                    description={t("requireDeviceVerificationHint")}
                    checked={loginForm.require_device_verification}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("require_device_verification", value)
                    }
                  />
                  <SecuritySwitch
                    title={t("allowTrustedLoginDevices")}
                    description={t("allowTrustedLoginDevicesHint")}
                    checked={loginForm.allow_trusted_login_devices}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("allow_trusted_login_devices", value)
                    }
                  />
                </div>
              </section>

              <section className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                <div className="mb-4 flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                  <div className="flex items-start gap-3">
                    <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-elevated text-kumo-brand">
                      <EnvelopeSimple size={18} />
                    </div>
                    <div>
                      <h3 className="text-sm font-semibold">
                        {t("emailSettings")}
                      </h3>
                      <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
                        {t("emailSettingsHint")}
                      </p>
                    </div>
                  </div>
                  <Button
                    type="button"
                    variant="secondary"
                    className="w-full lg:w-auto"
                    onClick={startNewSmtpConfig}
                  >
                    <Plus size={16} />
                    {t("addSmtpConfig")}
                  </Button>
                </div>

                <div className="grid gap-4 lg:grid-cols-[minmax(220px,320px)_minmax(0,1fr)]">
                  <div className="space-y-2">
                    {smtpConfigs.data?.length === 0 && (
                      <div className="rounded-lg border border-dashed border-kumo-line bg-kumo-elevated px-3 py-4 text-sm leading-6 text-kumo-subtle">
                        {t("noSmtpConfigs")}
                      </div>
                    )}
                    {smtpConfigs.data?.map((config) => {
                      const selected = config.id === emailForm.id;
                      return (
                        <button
                          key={config.id}
                          type="button"
                          className={[
                            "w-full rounded-lg border px-3 py-3 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand",
                            selected
                              ? "border-kumo-brand bg-kumo-brand/10"
                              : "border-kumo-line bg-kumo-elevated hover:bg-kumo-tint/60",
                          ].join(" ")}
                          onClick={() => selectSmtpConfig(config)}
                        >
                          <span className="flex items-center justify-between gap-2">
                            <span className="min-w-0 truncate text-sm font-semibold">
                              {config.name || config.host || t("smtpUnnamedConfig")}
                            </span>
                            {config.enabled && (
                              <span className="inline-flex shrink-0 items-center gap-1 rounded-md bg-kumo-success/10 px-2 py-0.5 text-xs font-medium text-kumo-success">
                                <CheckCircle size={12} weight="fill" />
                                {t("activeSmtpConfig")}
                              </span>
                            )}
                          </span>
                          <span className="mt-1 block truncate text-xs text-kumo-subtle">
                            {config.host || t("emptyValue")}
                            {config.from ? ` / ${config.from}` : ""}
                          </span>
                        </button>
                      );
                    })}
                  </div>

                  <div className="rounded-lg border border-kumo-line bg-kumo-elevated p-4">
                    <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                      <div>
                        <h4 className="text-sm font-semibold">
                          {emailForm.id ? t("editSmtpConfig") : t("newSmtpConfig")}
                        </h4>
                        <p className="mt-1 text-xs leading-5 text-kumo-subtle">
                          {emailForm.id && selectedSmtpConfig?.enabled
                            ? t("activeSmtpConfigHint")
                            : t("smtpConfigFormHint")}
                        </p>
                      </div>
                      {emailForm.id > 0 && !selectedSmtpConfig?.enabled && (
                        <Button
                          type="button"
                          variant="secondary"
                          className="w-full sm:w-auto"
                          loading={enableSmtp.isPending}
                          disabled={enableSmtp.isPending}
                          onClick={() => enableSmtp.mutate(emailForm.id)}
                        >
                          <CheckCircle size={16} />
                          {t("enableSmtpConfig")}
                        </Button>
                      )}
                    </div>

                    <div className="grid gap-4 md:grid-cols-2">
                      <label className="block md:col-span-2">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("smtpConfigName")}
                        </span>
                        <Input
                          aria-label={t("smtpConfigName")}
                          value={emailForm.name}
                          maxLength={80}
                          placeholder={t("smtpConfigNamePlaceholder")}
                          onChange={(e) => updateEmail("name", e.target.value)}
                        />
                      </label>
                      <label className="block md:col-span-2">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("smtpHost")}
                        </span>
                        <Input
                          aria-label={t("smtpHost")}
                          value={emailForm.host}
                          maxLength={255}
                          onChange={(e) => updateEmail("host", e.target.value)}
                        />
                      </label>
                      <label className="block">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("smtpPort")}
                        </span>
                        <Input
                          aria-label={t("smtpPort")}
                          type="number"
                          value={String(emailForm.port)}
                          onChange={(e) =>
                            updateEmail("port", Number(e.target.value) || 0)
                          }
                        />
                      </label>
                      <label className="block">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("smtpTls")}
                        </span>
                        <select
                          className="h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                          value={emailForm.tls}
                          onChange={(e) => updateEmail("tls", e.target.value)}
                        >
                          <option value="starttls">{t("smtpTlsStarttls")}</option>
                          <option value="tls">{t("smtpTlsTls")}</option>
                          <option value="none">{t("smtpTlsNone")}</option>
                        </select>
                      </label>
                      <label className="block">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("smtpUsername")}
                        </span>
                        <Input
                          aria-label={t("smtpUsername")}
                          value={emailForm.username}
                          maxLength={255}
                          autoComplete="username"
                          onChange={(e) => updateEmail("username", e.target.value)}
                        />
                      </label>
                      <label className="block">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("smtpPassword")}
                        </span>
                        <Input
                          aria-label={t("smtpPassword")}
                          type="password"
                          value={emailForm.password}
                          maxLength={500}
                          autoComplete="new-password"
                          placeholder={
                            selectedSmtpConfig?.password_set
                              ? t("secretConfiguredPlaceholder")
                              : t("secretEmptyPlaceholder")
                          }
                          onChange={(e) => updateEmail("password", e.target.value)}
                        />
                      </label>
                      <label className="block md:col-span-2">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("smtpFrom")}
                        </span>
                        <Input
                          aria-label={t("smtpFrom")}
                          type="email"
                          value={emailForm.from}
                          maxLength={255}
                          onChange={(e) => updateEmail("from", e.target.value)}
                        />
                      </label>
                      {selectedSmtpConfig?.password_set && (
                        <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-2 md:col-span-2">
                          <Switch
                            label={t("clearConfiguredSecret")}
                            controlFirst={false}
                            checked={emailForm.clear_password}
                            onCheckedChange={(value: boolean) =>
                              updateEmail("clear_password", value)
                            }
                          />
                        </div>
                      )}
                      <label className="block md:col-span-2">
                        <span className="mb-1.5 block text-sm font-medium">
                          {t("testEmailRecipient")}
                        </span>
                        <div className="flex flex-col gap-2 sm:flex-row">
                          <Input
                            aria-label={t("testEmailRecipient")}
                            type="email"
                            value={testEmail}
                            placeholder={t("testEmailRecipientPlaceholder")}
                            className="min-w-0 flex-1"
                            onChange={(e) => setTestEmail(e.target.value)}
                          />
                          <Button
                            type="button"
                            variant="secondary"
                            loading={testEmailMutation.isPending}
                            disabled={
                              testEmailMutation.isPending || emailForm.id <= 0
                            }
                            onClick={() => testEmailMutation.mutate()}
                          >
                            <PaperPlaneTilt size={16} />
                            {t("sendTestEmail")}
                          </Button>
                        </div>
                      </label>
                    </div>

                    <div className="mt-5 flex flex-col-reverse gap-2 border-t border-kumo-line pt-4 sm:flex-row sm:items-center sm:justify-between">
                      <div>
                        {emailForm.id > 0 && (
                          <Button
                            type="button"
                            variant="secondary-destructive"
                            className="w-full sm:w-auto"
                            disabled={deleteSmtp.isPending}
                            onClick={() => {
                              if (selectedSmtpConfig) setSmtpDeleteTarget(selectedSmtpConfig);
                            }}
                          >
                            <Trash size={16} />
                            {t("deleteSmtpConfig")}
                          </Button>
                        )}
                      </div>
                      <Button
                        type="button"
                        className="w-full sm:w-auto"
                        loading={saveSmtp.isPending}
                        disabled={saveSmtp.isPending}
                        onClick={saveSmtpConfig}
                      >
                        <EnvelopeSimple size={16} />
                        {t("saveSmtpConfig")}
                      </Button>
                    </div>
                  </div>
                </div>
              </section>
            </div>
          )}
        </section>
      )}

      <ConfirmDialog
        open={smtpDeleteTarget !== null}
        title={t("confirmDeleteSmtpConfigTitle")}
        description={t("confirmDeleteSmtpConfigDescription")}
        confirmLabel={t("deleteSmtpConfig")}
        cancelLabel={t("cancel")}
        loading={deleteSmtp.isPending}
        error={
          deleteSmtp.error
            ? (deleteSmtp.error as Error).message || t("operationFailed")
            : undefined
        }
        onOpenChange={(open) => {
          if (!open) {
            setSmtpDeleteTarget(null);
            deleteSmtp.reset();
          }
        }}
        onConfirm={() => {
          if (smtpDeleteTarget) deleteSmtp.mutate(smtpDeleteTarget.id);
        }}
      />

      <Dialog.Root open={previewOpen} onOpenChange={setPreviewOpen}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader
            title={t("siteWelcomePreview")}
            description={t("siteWelcomePreviewHint")}
          />
          <DialogBody>
            <div className="min-h-40 whitespace-pre-wrap break-words rounded-lg border border-kumo-line bg-kumo-base px-4 py-3 text-sm leading-6">
              {preview || t("emptyValue")}
            </div>
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={() => setPreviewOpen(false)}>
              {t("close")}
            </Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>
    </div>
  );
}

function SecuritySwitch({
  title,
  description,
  checked,
  onCheckedChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}) {
  return (
    <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-3">
      <Switch
        label={title}
        controlFirst={false}
        checked={checked}
        onCheckedChange={onCheckedChange}
      />
      <p className="mt-1 text-sm leading-6 text-kumo-subtle">{description}</p>
    </div>
  );
}
