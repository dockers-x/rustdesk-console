import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { apiPost, ApiError } from "../lib/api";
import { setToken } from "../lib/auth";

interface AdminLoginPayload {
  token: string;
}

export function RegisterPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    if (password !== confirmPassword) {
      setError(t("passwordMismatch"));
      return;
    }
    setLoading(true);
    try {
      const res = await apiPost<AdminLoginPayload>("/api/admin/user/register", {
        username,
        email,
        password,
        confirm_password: confirmPassword,
      });
      setToken(res.token);
      navigate("/", { replace: true });
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center bg-kumo-base px-4 text-kumo-default">
      <form
        onSubmit={submit}
        className="w-full max-w-[380px] rounded-xl border border-kumo-line bg-kumo-elevated p-8 shadow-lg"
      >
        <h1 className="mb-6 text-center text-xl font-semibold">
          {t("register")}
        </h1>
        <div className="space-y-4">
          <label className="block">
            <span className="mb-1 block text-sm">{t("username")}</span>
            <Input
              value={username}
              autoComplete="username"
              autoFocus
              onChange={(e) => setUsername(e.target.value)}
            />
          </label>
          <label className="block">
            <span className="mb-1 block text-sm">{t("email")}</span>
            <Input
              type="email"
              value={email}
              autoComplete="email"
              onChange={(e) => setEmail(e.target.value)}
            />
          </label>
          <label className="block">
            <span className="mb-1 block text-sm">{t("password")}</span>
            <Input
              type="password"
              value={password}
              autoComplete="new-password"
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
          <label className="block">
            <span className="mb-1 block text-sm">{t("confirmPassword")}</span>
            <Input
              type="password"
              value={confirmPassword}
              autoComplete="new-password"
              onChange={(e) => setConfirmPassword(e.target.value)}
            />
          </label>
          {error && <p className="text-sm text-red-500">{error}</p>}
          <Button type="submit" className="w-full" disabled={loading}>
            {t("submit")}
          </Button>
          <Link
            to="/login"
            className="block rounded-md px-3 py-2 text-center text-sm hover:bg-kumo-tint/60"
          >
            {t("toLogin")}
          </Link>
        </div>
      </form>
    </div>
  );
}
