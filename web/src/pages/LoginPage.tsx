import { useEffect, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { InlineMessage } from "../components/InlineMessage";
import {
  authenticatedHome,
  canAccessConsolePath,
  hasAdminAccess,
} from "../lib/access";
import { useAppBranding } from "../lib/adminTitle";
import { apiGet, apiPost, ApiError } from "../lib/api";
import { clearOidcCode, getOidcCode, setOidcCode, setToken } from "../lib/auth";

interface Captcha {
  id: string;
  b64: string;
}
interface LoginResult {
  token?: string;
  must_change_password?: boolean;
  type?: string;
  tfa_type?: string;
  secret?: string;
  route_names?: string[];
  user?: {
    username?: string;
    email?: string;
  };
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
interface LoginChallenge {
  secret: string;
  tfaType: string;
  username?: string;
  email?: string;
}

interface PasswordResetStart {
  secret: string;
}

type PasswordResetStep = "idle" | "request" | "confirm";

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
  const { title: appTitle, largeLogoUrl } = useAppBranding();
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
  const [challenge, setChallenge] = useState<LoginChallenge | null>(null);
  const [verificationCode, setVerificationCode] = useState("");
  const [resetStep, setResetStep] = useState<PasswordResetStep>("idle");
  const [resetAccount, setResetAccount] = useState("");
  const [resetSecret, setResetSecret] = useState("");
  const [resetCode, setResetCode] = useState("");
  const [resetPassword, setResetPassword] = useState("");
  const [resetConfirmPassword, setResetConfirmPassword] = useState("");

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
    if (!res.token) {
      setError(t("loginFailed"));
      return;
    }
    const changeRequired = Boolean(res.must_change_password);
    setToken(res.token, changeRequired);
    const isAdmin = hasAdminAccess(res.route_names);
    const target =
      loginRedirectPath && canAccessConsolePath(loginRedirectPath, isAdmin)
        ? loginRedirectPath
        : isAdmin
          ? fallbackPath
          : authenticatedHome(false);
    navigate(changeRequired ? "/change-password" : target, { replace: true });
  };

  const beginVerification = (res: LoginResult) => {
    if (res.type !== "email_check" || !res.secret) return false;
    setChallenge({
      secret: res.secret,
      tfaType: res.tfa_type || "email_check",
      username: res.user?.username || username,
      email: res.user?.email,
    });
    setVerificationCode("");
    setMessage(t(res.tfa_type === "tfa_check" ? "totpChallengeHint" : "emailChallengeHint"));
    return true;
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
      if (!beginVerification(res)) finishLogin(res, "/");
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
    if (challenge) {
      await submitChallenge();
      return;
    }
    setError("");
    setMessage("");
    setLoading(true);
    try {
      const res = await apiPost<LoginResult>("/api/admin/login", {
        username,
        password,
        captcha,
        captchaId: captchaInfo?.id ?? "",
        platform: platformName(),
      });
      if (beginVerification(res)) return;
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

  const submitChallenge = async () => {
    if (!challenge) return;
    const code = verificationCode.trim();
    if (!code) {
      setError(t("verificationCodeRequired"));
      return;
    }
    setError("");
    setMessage("");
    setLoading(true);
    try {
      const isTotp = challenge.tfaType === "tfa_check";
      const res = await apiPost<LoginResult>("/api/admin/login", {
        type: isTotp ? "tfa_check" : "email_code",
        secret: challenge.secret,
        verificationCode: isTotp ? "" : code,
        tfaCode: isTotp ? code : "",
        platform: platformName(),
      });
      finishLogin(res, "/overview");
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("loginFailed"));
    } finally {
      setLoading(false);
    }
  };

  const resetChallenge = () => {
    setChallenge(null);
    setVerificationCode("");
    setMessage("");
    setError("");
  };

  const startPasswordReset = () => {
    setChallenge(null);
    setVerificationCode("");
    setResetStep("request");
    setResetAccount(username);
    setResetSecret("");
    setResetCode("");
    setResetPassword("");
    setResetConfirmPassword("");
    setMessage("");
    setError("");
  };

  const backToLogin = () => {
    setResetStep("idle");
    setResetSecret("");
    setResetCode("");
    setResetPassword("");
    setResetConfirmPassword("");
    setMessage("");
    setError("");
  };

  const submitPasswordResetRequest = async (e: React.FormEvent) => {
    e.preventDefault();
    const account = resetAccount.trim();
    if (!account) {
      setError(t("accountRequired"));
      return;
    }
    setError("");
    setMessage("");
    setLoading(true);
    try {
      const res = await apiPost<PasswordResetStart>(
        "/api/admin/password-reset/request",
        { account },
      );
      setResetSecret(res.secret);
      setResetStep("confirm");
      setMessage(t("passwordResetCodeSent"));
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  const submitPasswordResetConfirm = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!resetCode.trim()) {
      setError(t("verificationCodeRequired"));
      return;
    }
    const code = resetCode.trim();
    const mailedToken = resetSecret ? null : /^([a-zA-Z0-9]{40})[.]([0-9]{6})$/.exec(code);
    if (!resetSecret && !mailedToken) {
      setError(t("passwordResetTokenInvalid"));
      return;
    }
    if (resetPassword.length < 4) {
      setError(t("passwordMinLength"));
      return;
    }
    if (resetPassword !== resetConfirmPassword) {
      setError(t("passwordMismatch"));
      return;
    }
    setError("");
    setMessage("");
    setLoading(true);
    try {
      await apiPost("/api/admin/password-reset/confirm", {
        secret: mailedToken?.[1] ?? resetSecret,
        code: mailedToken?.[2] ?? code,
        password: resetPassword,
        confirmPassword: resetConfirmPassword,
      });
      setPassword("");
      setResetStep("idle");
      setResetSecret("");
      setResetCode("");
      setResetPassword("");
      setResetConfirmPassword("");
      setMessage(t("passwordResetDone"));
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
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

  const submitActiveForm = (e: React.FormEvent) => {
    if (setupMode) return void submitSetup(e);
    if (resetStep === "request") return void submitPasswordResetRequest(e);
    if (resetStep === "confirm") return void submitPasswordResetConfirm(e);
    return void submit(e);
  };

  const formTitle = setupMode
    ? t("initialSetup")
    : resetStep !== "idle"
      ? t("passwordReset")
      : challenge
        ? t("loginVerification")
        : t("login");
  const formSubtitle = setupMode
    ? t("initialSetupSubtitle")
    : resetStep !== "idle"
      ? t("passwordResetSubtitle")
      : challenge
        ? t("loginVerificationSubtitle")
        : null;

  return (
    <div className="relative min-h-full overflow-auto bg-kumo-base px-4 py-6 text-kumo-default sm:px-6 lg:px-8">
      <div className="login-grid-bg pointer-events-none absolute inset-0 opacity-60" />
      <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-kumo-brand/70" />
      <div className="relative z-10 mx-auto grid min-h-[calc(100dvh-3rem)] w-full max-w-6xl items-center">
        <div className="grid overflow-hidden rounded-lg border border-kumo-line bg-kumo-elevated shadow-lg lg:grid-cols-[minmax(0,1fr)_420px]">
          <section className="login-brand relative isolate min-w-0 overflow-hidden p-6 sm:p-8 lg:flex lg:flex-col lg:justify-center lg:p-10">
            <img src={largeLogoUrl} alt="" className="login-brand-logo size-14 object-contain" />
            <h1 className="mt-5 max-w-xl break-words text-3xl font-semibold leading-tight sm:text-4xl">
              {appTitle}
            </h1>
            <p className="mt-4 max-w-md text-sm leading-6 text-kumo-subtle sm:text-base">
              {t("loginSubtitle")}
            </p>
          </section>

          <form
            onSubmit={submitActiveForm}
            className="auth-form border-t border-kumo-line bg-kumo-base p-6 sm:p-8 lg:border-l lg:border-t-0 lg:p-10"
          >
            <div className="mb-6">
              <h2 className="text-2xl font-semibold">{formTitle}</h2>
              {formSubtitle && <p className="mt-2 text-sm text-kumo-subtle">{formSubtitle}</p>}
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
              {setupChecked && !setupMode && resetStep === "idle" && challenge && (
                <>
                  <div className="rounded-lg border border-kumo-line bg-kumo-elevated p-3 text-sm leading-6">
                    <div className="font-medium">
                      {challenge.tfaType === "tfa_check"
                        ? t("totpVerification")
                        : t("emailVerification")}
                    </div>
                    <div className="mt-1 break-words text-kumo-subtle">
                      {challenge.tfaType === "tfa_check"
                        ? t("totpChallengeDescription")
                        : t("emailChallengeDescription", {
                            email: challenge.email || challenge.username || t("account"),
                          })}
                    </div>
                  </div>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("verificationCode")}
                    </span>
                    <Input
                      aria-label={t("verificationCode")}
                      value={verificationCode}
                      inputMode="numeric"
                      autoComplete="one-time-code"
                      autoFocus
                      className="w-full"
                      onChange={(e) => {
                        setVerificationCode(e.target.value);
                        setError("");
                      }}
                    />
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
                    {t("verifyAndLogin")}
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    className="w-full justify-center"
                    disabled={loading}
                    onClick={resetChallenge}
                  >
                    {t("backToLogin")}
                  </Button>
                </>
              )}
              {setupChecked && !setupMode && resetStep === "request" && (
                <>
                  <p className="rounded-md border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm leading-6 text-kumo-subtle">
                    {t("passwordResetRequestHint")}
                  </p>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("account")}
                    </span>
                    <Input
                      aria-label={t("account")}
                      value={resetAccount}
                      onChange={(e) => {
                        setResetAccount(e.target.value);
                        setError("");
                      }}
                      autoComplete="username"
                      autoFocus
                      className="w-full"
                    />
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
                    {t("sendResetCode")}
                  </Button>
                  <Button type="button" variant="secondary" className="w-full justify-center"
                    disabled={loading} onClick={() => {
                      setError("");
                      setMessage("");
                      setResetSecret("");
                      setResetCode("");
                      setResetStep("confirm");
                    }}>
                    {t("passwordResetUseToken")}
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    className="w-full justify-center"
                    disabled={loading}
                    onClick={backToLogin}
                  >
                    {t("backToLogin")}
                  </Button>
                </>
              )}
              {setupChecked && !setupMode && resetStep === "confirm" && (
                <>
                  <div className="rounded-lg border border-kumo-line bg-kumo-elevated p-3 text-sm leading-6">
                    <div className="font-medium">{t(resetSecret ? "passwordResetCodeTitle" : "passwordResetUseToken")}</div>
                    <div className="mt-1 text-kumo-subtle">
                      {t(resetSecret ? "passwordResetConfirmHint" : "passwordResetTokenHint")}
                    </div>
                  </div>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t(resetSecret ? "verificationCode" : "passwordResetToken")}
                    </span>
                    <Input
                      aria-label={t(resetSecret ? "verificationCode" : "passwordResetToken")}
                      value={resetCode}
                      inputMode={resetSecret ? "numeric" : "text"}
                      type={resetSecret ? "text" : "password"}
                      autoComplete="one-time-code"
                      autoFocus
                      className="w-full"
                      onChange={(e) => {
                        setResetCode(e.target.value);
                        setError("");
                      }}
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("newPassword")}
                    </span>
                    <Input
                      aria-label={t("newPassword")}
                      type="password"
                      value={resetPassword}
                      autoComplete="new-password"
                      className="w-full"
                      onChange={(e) => {
                        setResetPassword(e.target.value);
                        setError("");
                      }}
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1.5 block text-sm font-medium">
                      {t("confirmPassword")}
                    </span>
                    <Input
                      aria-label={t("confirmPassword")}
                      type="password"
                      value={resetConfirmPassword}
                      autoComplete="new-password"
                      className="w-full"
                      onChange={(e) => {
                        setResetConfirmPassword(e.target.value);
                        setError("");
                      }}
                    />
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
                    {t("resetPassword")}
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    className="w-full justify-center"
                    disabled={loading}
                    onClick={backToLogin}
                  >
                    {t("backToLogin")}
                  </Button>
                </>
              )}
              {setupChecked && !setupMode && resetStep === "idle" && !challenge && !options.disable_pwd && (
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
              {setupChecked && !setupMode && resetStep === "idle" && !challenge && !options.disable_pwd && captchaInfo && (
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
              {setupChecked && !setupMode && resetStep === "idle" && !challenge && (
                <>
                  {message && (
                    <InlineMessage tone="success">{message}</InlineMessage>
                  )}
                  {error && <InlineMessage tone="error">{error}</InlineMessage>}
                  {!options.disable_pwd && (
                    <>
                      <Button
                        type="submit"
                        className="w-full justify-center"
                        disabled={loading}
                        loading={loading}
                      >
                        {t("login")}
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        className="w-full justify-center"
                        disabled={loading}
                        onClick={startPasswordReset}
                      >
                        {t("forgotPassword")}
                      </Button>
                    </>
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
