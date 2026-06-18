import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Switch } from "@cloudflare/kumo/components/switch";
import {
  EnvelopeSimple,
  Eye,
  GearSix,
  Info,
  NotePencil,
  PaperPlaneTilt,
  ShieldCheck,
} from "@phosphor-icons/react";
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
  host: string;
  port: number;
  username: string;
  from: string;
  tls: string;
  password_set: boolean;
  configured: boolean;
}
interface LoginSecurityConfigView {
  login: LoginSecuritySettings;
  email: EmailSettingsView;
}
interface EmailSettingsForm {
  host: string;
  port: number;
  username: string;
  password: string;
  clear_password: boolean;
  from: string;
  tls: string;
}
interface LoginSecurityForm {
  login: LoginSecuritySettings;
  email: EmailSettingsForm;
}
type SettingsTab = "site" | "security";

const emptyForm: AdminConfigForm = {
  title: "",
  hello: "",
  hello_file: "",
};
const emptySecurityForm: LoginSecurityForm = {
  login: {
    require_totp: false,
    require_email_verification: false,
    require_device_verification: false,
    allow_trusted_login_devices: true,
  },
  email: {
    host: "",
    port: 587,
    username: "",
    password: "",
    clear_password: false,
    from: "",
    tls: "starttls",
  },
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
): LoginSecurityForm {
  return {
    login: {
      ...emptySecurityForm.login,
      ...(config.login ?? {}),
    },
    email: {
      ...emptySecurityForm.email,
      ...(config.email
        ? {
            host: config.email.host,
            port: config.email.port || 587,
            username: config.email.username,
            from: config.email.from,
            tls: config.email.tls || "starttls",
          }
        : {}),
      password: "",
      clear_password: false,
    },
  };
}

export function SystemSettingsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [form, setForm] = useState<AdminConfigForm>(emptyForm);
  const [securityForm, setSecurityForm] =
    useState<LoginSecurityForm>(emptySecurityForm);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [securityMessage, setSecurityMessage] = useState("");
  const [securityError, setSecurityError] = useState("");
  const [testEmail, setTestEmail] = useState("");
  const [activeTab, setActiveTab] = useState<SettingsTab>("site");
  const [previewOpen, setPreviewOpen] = useState(false);

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

  useEffect(() => {
    if (config.data) setForm(normalizeConfig(config.data));
  }, [config.data]);
  useEffect(() => {
    if (loginSecurity.data) {
      setSecurityForm(normalizeSecurityConfig(loginSecurity.data));
    }
  }, [loginSecurity.data]);

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
    mutationFn: (payload: LoginSecurityForm) =>
      apiPatch<LoginSecurityConfigView>("/api/admin/config/login-security", payload),
    onSuccess: (saved) => {
      setSecurityForm(normalizeSecurityConfig(saved));
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
  const testEmailMutation = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/config/login-security/test-email", {
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
    setSecurityForm((current) => ({
      ...current,
      login: { ...current.login, [field]: value },
    }));
    setSecurityMessage("");
    setSecurityError("");
  };
  const updateEmail = <K extends keyof EmailSettingsForm>(
    field: K,
    value: EmailSettingsForm[K],
  ) => {
    setSecurityForm((current) => ({
      ...current,
      email: { ...current.email, [field]: value },
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
  const emailConfigured = Boolean(
    securityForm.email.host.trim() && securityForm.email.from.trim(),
  );
  const emailRequired =
    securityForm.login.require_email_verification ||
    securityForm.login.require_device_verification;
  const saveSiteSettings = () =>
    save.mutate({
      title: form.title.trim(),
      hello: form.hello.trim(),
      hello_file: form.hello_file.trim(),
    });
  const saveLoginSecurity = () =>
    saveSecurity.mutate({
      login: securityForm.login,
      email: {
        ...securityForm.email,
        host: securityForm.email.host.trim(),
        username: securityForm.email.username.trim(),
        from: securityForm.email.from.trim(),
        password: securityForm.email.password,
      },
    });
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
        <Button
          loading={activeTab === "site" ? save.isPending : saveSecurity.isPending}
          onClick={activeTab === "site" ? saveSiteSettings : saveLoginSecurity}
        >
          <GearSix size={16} />
          {activeTab === "site"
            ? t("writeSystemSettings")
            : t("saveLoginSecurity")}
        </Button>
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

          {loginSecurity.isLoading && (
            <TableState tone="loading">{t("loading")}</TableState>
          )}
          {loginSecurity.error && (
            <TableState tone="error">
              {(loginSecurity.error as Error).message || t("operationFailed")}
            </TableState>
          )}

          {!loginSecurity.isLoading && !loginSecurity.error && (
            <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_420px]">
              <section className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                <div className="mb-4">
                  <h3 className="text-sm font-semibold">
                    {t("loginSecurityPolicy")}
                  </h3>
                  <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                    {t("loginSecurityPolicyHint")}
                  </p>
                </div>
                <div className="grid gap-3 md:grid-cols-2">
                  <SecuritySwitch
                    title={t("requireTotp")}
                    description={t("requireTotpHint")}
                    checked={securityForm.login.require_totp}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("require_totp", value)
                    }
                  />
                  <SecuritySwitch
                    title={t("requireEmailVerification")}
                    description={t("requireEmailVerificationHint")}
                    checked={securityForm.login.require_email_verification}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("require_email_verification", value)
                    }
                  />
                  <SecuritySwitch
                    title={t("requireDeviceVerification")}
                    description={t("requireDeviceVerificationHint")}
                    checked={securityForm.login.require_device_verification}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("require_device_verification", value)
                    }
                  />
                  <SecuritySwitch
                    title={t("allowTrustedLoginDevices")}
                    description={t("allowTrustedLoginDevicesHint")}
                    checked={securityForm.login.allow_trusted_login_devices}
                    onCheckedChange={(value) =>
                      updateLoginSecurity("allow_trusted_login_devices", value)
                    }
                  />
                </div>
              </section>

              <div className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                <div className="mb-4 flex items-start gap-3">
                  <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-elevated text-kumo-brand">
                    <EnvelopeSimple size={18} />
                  </div>
                  <div>
                    <h3 className="text-sm font-semibold">
                      {t("emailSettings")}
                    </h3>
                    <p className="mt-1 text-sm leading-6 text-kumo-subtle">
                      {t("emailSettingsHint")}
                    </p>
                  </div>
                </div>
                <div className="grid gap-4 sm:grid-cols-2">
                  <label className="block sm:col-span-2">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("smtpHost")}
                    </span>
                    <Input
                      aria-label={t("smtpHost")}
                      value={securityForm.email.host}
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
                      value={String(securityForm.email.port)}
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
                      value={securityForm.email.tls}
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
                      value={securityForm.email.username}
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
                      value={securityForm.email.password}
                      maxLength={500}
                      autoComplete="new-password"
                      placeholder={
                        loginSecurity.data?.email.password_set
                          ? t("secretConfiguredPlaceholder")
                          : t("secretEmptyPlaceholder")
                      }
                      onChange={(e) => updateEmail("password", e.target.value)}
                    />
                  </label>
                  <label className="block sm:col-span-2">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("smtpFrom")}
                    </span>
                    <Input
                      aria-label={t("smtpFrom")}
                      type="email"
                      value={securityForm.email.from}
                      maxLength={255}
                      onChange={(e) => updateEmail("from", e.target.value)}
                    />
                  </label>
                  {loginSecurity.data?.email.password_set && (
                    <div className="rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 sm:col-span-2">
                      <Switch
                        label={t("clearConfiguredSecret")}
                        controlFirst={false}
                        checked={securityForm.email.clear_password}
                        onCheckedChange={(value: boolean) =>
                          updateEmail("clear_password", value)
                        }
                      />
                    </div>
                  )}
                  <label className="block sm:col-span-2">
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
                        disabled={testEmailMutation.isPending}
                        onClick={() => testEmailMutation.mutate()}
                      >
                        <PaperPlaneTilt size={16} />
                        {t("sendTestEmail")}
                      </Button>
                    </div>
                  </label>
                </div>
              </div>
            </div>
          )}
        </section>
      )}

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
