import { useEffect, useState, type ComponentType } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { cn } from "@cloudflare/kumo/utils";
import {
  Key,
  Monitor,
  PlugsConnected,
  ShieldCheck,
} from "@phosphor-icons/react";
import { InlineMessage } from "../components/InlineMessage";
import { useAppTitle } from "../lib/adminTitle";
import { apiGet, apiPost, ApiError } from "../lib/api";
import { clearOidcCode, getOidcCode, setOidcCode, setToken } from "../lib/auth";

interface Captcha {
  id: string;
  b64: string;
}
interface LoginResult {
  token: string;
  must_change_password?: boolean;
}
interface LoginOptions {
  ops: string[];
  register: boolean;
  need_captcha: boolean;
  disable_pwd: boolean;
  auto_oidc: boolean;
}
interface OidcStart {
  code: string;
  url: string;
}
interface SetupStatus {
  initialized: boolean;
  can_initialize: boolean;
  title?: string;
}

type SignalIcon = ComponentType<{
  size?: number;
  weight?: "regular" | "fill";
  className?: string;
}>;

function AuthSignal({
  icon: Icon,
  label,
  value,
  active,
}: {
  icon: SignalIcon;
  label: string;
  value: string;
  active: boolean;
}) {
  return (
    <div className="flex items-center gap-3 border-t border-kumo-line py-3 first:border-t-0">
      <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-subtle">
        <Icon size={18} weight={active ? "fill" : "regular"} aria-hidden />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{label}</div>
        <div className="mt-0.5 truncate text-xs text-kumo-subtle">{value}</div>
      </div>
      <span
        className={cn(
          "size-2 rounded-full",
          active ? "bg-kumo-success" : "bg-kumo-fill",
        )}
        aria-hidden="true"
      />
    </div>
  );
}

function platformName() {
  const p = window.navigator.platform;
  if (p.startsWith("Mac")) return "mac";
  if (p.startsWith("Win")) return "windows";
  if (p.startsWith("Linux armv")) return "android";
  if (p.startsWith("Linux")) return "linux";
  return p || "web";
}

function browserName() {
  const ua = navigator.userAgent;
  if (/edg/i.test(ua)) return "Edge";
  if (/chrome|crios/i.test(ua)) return "Chrome";
  if (/firefox|fxios/i.test(ua)) return "Firefox";
  if (/safari/i.test(ua) && !/chrome/i.test(ua)) return "Safari";
  return "Browser";
}

export function LoginPage() {
  const { t } = useTranslation();
  const appTitle = useAppTitle();
  const navigate = useNavigate();
  const location = useLocation();
  const locationState = location.state as {
    message?: string;
    from?: string;
  } | null;
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [captcha, setCaptcha] = useState("");
  const [captchaInfo, setCaptchaInfo] = useState<Captcha | null>(null);
  const [options, setOptions] = useState<LoginOptions>({
    ops: [],
    register: false,
    need_captcha: false,
    disable_pwd: false,
    auto_oidc: false,
  });
  const [error, setError] = useState("");
  const [message, setMessage] = useState(locationState?.message ?? "");
  const [loading, setLoading] = useState(false);
  const [setupChecked, setSetupChecked] = useState(false);
  const [setupMode, setSetupMode] = useState(false);
  const [setupUsername, setSetupUsername] = useState("admin");
  const [setupNickname, setSetupNickname] = useState("");
  const [setupEmail, setSetupEmail] = useState("");
  const [setupPassword, setSetupPassword] = useState("");
  const [setupConfirmPassword, setSetupConfirmPassword] = useState("");

  const passwordEnabled = !options.disable_pwd;
  const oidcEnabled = options.ops.length > 0;
  const captchaEnabled = Boolean(options.need_captcha || captchaInfo);

  const loadCaptcha = async () => {
    try {
      const res = await apiGet<{ captcha: Captcha }>("/api/admin/captcha");
      setCaptchaInfo(res.captcha);
    } catch {
      /* captcha not required */
    }
  };

  const loginRedirectPath =
    locationState?.from && locationState.from !== "/login"
      ? locationState.from
      : "";

  const finishLogin = (res: LoginResult, fallbackPath: string) => {
    const changeRequired = Boolean(res.must_change_password);
    setToken(res.token, changeRequired);
    const target = loginRedirectPath || fallbackPath;
    navigate(changeRequired ? "/change-password" : target, { replace: true });
  };

  const queryOidc = async (code: string) => {
    setError("");
    setMessage("");
    setLoading(true);
    try {
      const res = await apiGet<LoginResult>("/api/admin/oidc/auth-query", {
        code,
      });
      clearOidcCode();
      finishLogin(res, "/");
    } catch (err) {
      clearOidcCode();
      const ae = err as ApiError;
      setError(ae.message || t("loginFailed"));
    } finally {
      setLoading(false);
    }
  };

  const startOidc = async (op: string) => {
    setError("");
    setMessage("");
    setLoading(true);
    try {
      const platform = platformName();
      const browser = browserName();
      const res = await apiPost<OidcStart>("/api/admin/oidc/auth", {
        op,
        id: `${platform}-${browser}`,
        uuid: "",
        deviceInfo: {
          name: navigator.userAgent,
          os: platform,
          type: "webadmin",
        },
      });
      setOidcCode(res.code);
      window.location.href = res.url;
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("loginFailed"));
      setLoading(false);
    }
  };

  const loadOptions = async () => {
    try {
      const res = await apiGet<LoginOptions>("/api/admin/login-options");
      setOptions(res);
      if (res.need_captcha) await loadCaptcha();
      if (res.auto_oidc && res.ops.length === 1) {
        await startOidc(res.ops[0]);
      }
    } catch {
      /* login options are optional for older servers */
    }
  };

  useEffect(() => {
    let mounted = true;
    const boot = async () => {
      const code = getOidcCode();
      try {
        const setup = await apiGet<SetupStatus>("/api/admin/setup/status");
        if (!mounted) return;
        if (setup.can_initialize) {
          setSetupMode(true);
          setSetupChecked(true);
          return;
        }
      } catch {
        /* older servers do not expose setup status */
      }

      if (code) {
        await queryOidc(code);
      } else {
        await loadOptions();
      }
      if (mounted) setSetupChecked(true);
    };

    void boot();
    return () => {
      mounted = false;
    };
  }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setMessage("");
    setLoading(true);
    try {
      const res = await apiPost<LoginResult>("/api/admin/login", {
        username,
        password,
        captcha,
        captchaId: captchaInfo?.id ?? "",
      });
      finishLogin(res, "/overview");
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("loginFailed"));
      // code 110 => captcha required
      if (ae.code === 110 || ae.code === 100) {
        await loadCaptcha();
      }
    } finally {
      setLoading(false);
    }
  };

  const submitSetup = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setMessage("");
    const nextUsername = setupUsername.trim();
    if (nextUsername.length < 2 || setupPassword.length < 4) {
      setError(t("setupValidationHint"));
      return;
    }
    if (setupPassword !== setupConfirmPassword) {
      setError(t("passwordMismatch"));
      return;
    }
    setLoading(true);
    try {
      const res = await apiPost<LoginResult>("/api/admin/setup/initialize", {
        username: nextUsername,
        nickname: setupNickname.trim(),
        email: setupEmail.trim(),
        password: setupPassword,
        confirmPassword: setupConfirmPassword,
      });
      finishLogin(res, "/overview");
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="relative min-h-full overflow-auto bg-kumo-base px-4 py-6 text-kumo-default sm:px-6 lg:px-8">
      <div className="login-grid-bg pointer-events-none absolute inset-0 opacity-60" />
      <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-kumo-brand/70" />
      <div className="relative z-10 mx-auto grid min-h-[calc(100dvh-3rem)] w-full max-w-6xl items-center">
        <div className="grid overflow-hidden rounded-lg border border-kumo-line bg-kumo-elevated shadow-lg lg:grid-cols-[minmax(0,1fr)_420px]">
          <section className="relative overflow-hidden p-6 sm:p-8 lg:p-10">
            <div className="relative flex h-full min-h-[180px] flex-col justify-between gap-6 sm:min-h-[240px] lg:min-h-[280px] lg:gap-10">
              <div>
                <div className="inline-flex min-h-9 items-center gap-2 rounded-lg border border-kumo-line bg-kumo-base px-3 text-xs font-semibold uppercase text-kumo-subtle">
                  <Monitor size={16} aria-hidden />
                  <span>{t("loginSurfaceTag")}</span>
                </div>
                <h1 className="mt-6 max-w-[16ch] break-words text-3xl font-semibold leading-tight sm:text-4xl lg:text-5xl">
                  {appTitle}
                </h1>
                <p className="mt-4 max-w-md text-sm leading-6 text-kumo-subtle sm:text-base">
                  {t("loginSubtitle")}
                </p>
              </div>

              <div className="hidden max-w-md border-y border-kumo-line lg:block">
                <AuthSignal
                  icon={PlugsConnected}
                  label={t("apiServer")}
                  value={t("available")}
                  active
                />
                {setupMode ? (
                  <>
                    <AuthSignal
                      icon={ShieldCheck}
                      label={t("initialSetup")}
                      value={t("setupAdminHint")}
                      active
                    />
                    <AuthSignal
                      icon={Key}
                      label={t("passwordAuth")}
                      value={t("enabled")}
                      active
                    />
                  </>
                ) : (
                  <>
                    <AuthSignal
                      icon={ShieldCheck}
                      label={t("passwordAuth")}
                      value={passwordEnabled ? t("enabled") : t("disabled")}
                      active={passwordEnabled}
                    />
                    <AuthSignal
                      icon={Key}
                      label={t("oauthAuth")}
                      value={oidcEnabled ? t("available") : t("notAvailable")}
                      active={oidcEnabled}
                    />
                    <AuthSignal
                      icon={Monitor}
                      label={t("captchaAuth")}
                      value={captchaEnabled ? t("enabled") : t("disabled")}
                      active={captchaEnabled}
                    />
                  </>
                )}
              </div>
            </div>
          </section>

          <form
            onSubmit={setupMode ? submitSetup : submit}
            className="border-t border-kumo-line bg-kumo-base p-6 sm:p-8 lg:border-l lg:border-t-0 lg:p-10"
          >
            <div className="mb-8">
              <div className="flex min-h-6 items-center gap-2 text-xs font-semibold uppercase text-kumo-subtle">
                <Key size={16} aria-hidden />
                <span>{setupMode ? t("setupWizardTag") : t("adminAccess")}</span>
              </div>
              <h2 className="mt-3 text-2xl font-semibold">
                {setupMode ? t("initialSetup") : t("login")}
              </h2>
              <p className="mt-2 text-sm text-kumo-subtle">
                {setupMode ? t("initialSetupSubtitle") : t("loginFormSubtitle")}
              </p>
            </div>

            <div className="space-y-4">
              {!setupChecked && (
                <p className="rounded-md border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm text-kumo-subtle">
                  {t("setupChecking")}
                </p>
              )}
              {setupChecked && setupMode && (
                <>
                  <p className="rounded-md border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm leading-6 text-kumo-subtle">
                    {t("setupAdminHint")}
                  </p>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("username")}
                    </span>
                    <Input
                      aria-label={t("username")}
                      value={setupUsername}
                      onChange={(e) => {
                        setSetupUsername(e.target.value);
                        setError("");
                      }}
                      autoComplete="username"
                      autoFocus
                      className="w-full"
                    />
                    <span className="mt-1.5 block text-xs text-kumo-subtle">
                      {t("setupUsernameHint")}
                    </span>
                  </label>
                  <div className="grid gap-4 sm:grid-cols-2">
                    <label className="block">
                      <span className="mb-1.5 block text-sm font-medium">
                        {t("nickname")}{" "}
                        <span className="font-normal text-kumo-subtle">
                          {t("optionalField")}
                        </span>
                      </span>
                      <Input
                        aria-label={t("nickname")}
                        value={setupNickname}
                        onChange={(e) => setSetupNickname(e.target.value)}
                        autoComplete="name"
                        className="w-full"
                      />
                    </label>
                    <label className="block">
                      <span className="mb-1.5 block text-sm font-medium">
                        {t("email")}{" "}
                        <span className="font-normal text-kumo-subtle">
                          {t("optionalField")}
                        </span>
                      </span>
                      <Input
                        aria-label={t("email")}
                        type="email"
                        value={setupEmail}
                        onChange={(e) => setSetupEmail(e.target.value)}
                        autoComplete="email"
                        className="w-full"
                      />
                    </label>
                  </div>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("password")}
                    </span>
                    <Input
                      aria-label={t("password")}
                      type="password"
                      value={setupPassword}
                      onChange={(e) => {
                        setSetupPassword(e.target.value);
                        setError("");
                      }}
                      autoComplete="new-password"
                      className="w-full"
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("confirmPassword")}
                    </span>
                    <Input
                      aria-label={t("confirmPassword")}
                      type="password"
                      value={setupConfirmPassword}
                      onChange={(e) => {
                        setSetupConfirmPassword(e.target.value);
                        setError("");
                      }}
                      autoComplete="new-password"
                      className="w-full"
                    />
                    <span className="mt-1.5 block text-xs text-kumo-subtle">
                      {t("setupValidationHint")}
                    </span>
                  </label>
                  {message && (
                    <InlineMessage tone="success">{message}</InlineMessage>
                  )}
                  {error && <InlineMessage tone="error">{error}</InlineMessage>}
                  <Button
                    type="submit"
                    className="w-full justify-center"
                    disabled={loading}
                    loading={loading}
                  >
                    {t("createAdmin")}
                  </Button>
                </>
              )}
              {setupChecked && !setupMode && !options.disable_pwd && (
                <>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("username")}
                    </span>
                    <Input
                      aria-label={t("username")}
                      value={username}
                      onChange={(e) => setUsername(e.target.value)}
                      autoComplete="username"
                      autoFocus
                      className="w-full"
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("password")}
                    </span>
                    <Input
                      aria-label={t("password")}
                      type="password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      autoComplete="current-password"
                      className="w-full"
                    />
                  </label>
                </>
              )}
              {setupChecked && !setupMode && !options.disable_pwd && captchaInfo && (
                <label className="block">
                  <span className="mb-1.5 block text-sm font-medium">
                    {t("captcha")}
                  </span>
                  <div className="flex items-center gap-2">
                    <Input
                      aria-label={t("captcha")}
                      value={captcha}
                      onChange={(e) => setCaptcha(e.target.value)}
                      className="min-w-0 flex-1"
                    />
                    <button
                      type="button"
                      aria-label={t("refreshCaptcha")}
                      onClick={() => void loadCaptcha()}
                      className="flex h-9 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-kumo-line bg-kumo-base transition hover:bg-kumo-tint focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                    >
                      <img
                        src={captchaInfo.b64}
                        alt={t("captcha")}
                        className="h-full"
                      />
                    </button>
                  </div>
                </label>
              )}
              {setupChecked && !setupMode && (
                <>
                  {message && (
                    <InlineMessage tone="success">{message}</InlineMessage>
                  )}
                  {error && <InlineMessage tone="error">{error}</InlineMessage>}
                  {!options.disable_pwd && (
                    <Button
                      type="submit"
                      className="w-full justify-center"
                      disabled={loading}
                      loading={loading}
                    >
                      {t("login")}
                    </Button>
                  )}
                  {options.register && (
                    <Link
                      to="/register"
                      className="block rounded-lg px-3 py-2 text-center text-sm transition hover:bg-kumo-tint/60 focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                    >
                      {t("register")}
                    </Link>
                  )}
                  {options.ops.length > 0 && !options.disable_pwd && (
                    <div className="flex items-center gap-2 text-xs text-kumo-subtle">
                      <span className="h-px flex-1 bg-kumo-line" />
                      <span>{t("orLoginWith")}</span>
                      <span className="h-px flex-1 bg-kumo-line" />
                    </div>
                  )}
                  {options.ops.map((op) => (
                    <Button
                      key={op}
                      type="button"
                      variant="secondary"
                      className="w-full justify-center"
                      disabled={loading}
                      loading={loading}
                      onClick={() => void startOidc(op)}
                    >
                      {op}
                    </Button>
                  ))}
                </>
              )}
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}
