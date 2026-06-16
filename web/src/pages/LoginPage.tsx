import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { apiGet, apiPost, ApiError } from "../lib/api";
import { clearOidcCode, getOidcCode, setOidcCode, setToken } from "../lib/auth";

interface Captcha {
  id: string;
  b64: string;
}
interface LoginResult {
  token: string;
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
  const navigate = useNavigate();
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
  const [loading, setLoading] = useState(false);

  const loadCaptcha = async () => {
    try {
      const res = await apiGet<{ captcha: Captcha }>("/api/admin/captcha");
      setCaptchaInfo(res.captcha);
    } catch {
      /* captcha not required */
    }
  };

  const queryOidc = async (code: string) => {
    setError("");
    setLoading(true);
    try {
      const res = await apiGet<LoginResult>("/api/admin/oidc/auth-query", {
        code,
      });
      clearOidcCode();
      setToken(res.token);
      navigate("/", { replace: true });
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
    const code = getOidcCode();
    if (code) {
      void queryOidc(code);
      return;
    }
    void loadOptions();
  }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const res = await apiPost<LoginResult>("/api/admin/login", {
        username,
        password,
        captcha,
        captchaId: captchaInfo?.id ?? "",
      });
      setToken(res.token);
      navigate("/users", { replace: true });
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

  return (
    <div className="flex h-full items-center justify-center bg-kumo-base text-kumo-default">
      <form
        onSubmit={submit}
        className="w-[360px] rounded-xl border border-kumo-line bg-kumo-elevated p-8 shadow-lg"
      >
        <h1 className="mb-6 text-center text-xl font-semibold">
          {t("appTitle")}
        </h1>
        <div className="space-y-4">
          {!options.disable_pwd && (
            <>
              <label className="block">
                <span className="mb-1 block text-sm">{t("username")}</span>
                <Input
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  autoComplete="username"
                  autoFocus
                />
              </label>
              <label className="block">
                <span className="mb-1 block text-sm">{t("password")}</span>
                <Input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  autoComplete="current-password"
                />
              </label>
            </>
          )}
          {!options.disable_pwd && captchaInfo && (
            <label className="block">
              <span className="mb-1 block text-sm">{t("captcha")}</span>
              <div className="flex items-center gap-2">
                <Input
                  value={captcha}
                  onChange={(e) => setCaptcha(e.target.value)}
                />
                <img
                  src={captchaInfo.b64}
                  alt="captcha"
                  className="h-9 cursor-pointer rounded"
                  onClick={loadCaptcha}
                />
              </div>
            </label>
          )}
          {error && <p className="text-sm text-red-500">{error}</p>}
          {!options.disable_pwd && (
            <Button type="submit" className="w-full" disabled={loading}>
              {t("login")}
            </Button>
          )}
          {options.register && (
            <Link
              to="/register"
              className="block rounded-md px-3 py-2 text-center text-sm hover:bg-kumo-tint/60"
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
              className="w-full"
              disabled={loading}
              onClick={() => void startOidc(op)}
            >
              {op}
            </Button>
          ))}
        </div>
      </form>
    </div>
  );
}
