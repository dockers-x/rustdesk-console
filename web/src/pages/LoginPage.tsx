import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { apiPost, ApiError } from "../lib/api";
import { setToken } from "../lib/auth";

interface Captcha {
  id: string;
  b64: string;
}
interface LoginResult {
  token: string;
}

export function LoginPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [captcha, setCaptcha] = useState("");
  const [captchaInfo, setCaptchaInfo] = useState<Captcha | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const loadCaptcha = async () => {
    try {
      const res = await apiPost<{ captcha: Captcha }>("/api/admin/captcha");
      setCaptchaInfo(res.captcha);
    } catch {
      /* captcha not required */
    }
  };

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
    <div className="flex h-full items-center justify-center bg-kumo-base text-color-surface">
      <form
        onSubmit={submit}
        className="w-[360px] rounded-xl border border-color-border bg-kumo-elevated p-8 shadow-lg"
      >
        <h1 className="mb-6 text-center text-xl font-semibold">
          {t("appTitle")}
        </h1>
        <div className="space-y-4">
          <label className="block">
            <span className="mb-1 block text-sm">{t("username")}</span>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoFocus
            />
          </label>
          <label className="block">
            <span className="mb-1 block text-sm">{t("password")}</span>
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
          {captchaInfo && (
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
          <Button type="submit" className="w-full" disabled={loading}>
            {t("login")}
          </Button>
        </div>
      </form>
    </div>
  );
}
