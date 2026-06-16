import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Key, ShieldCheck, SignOut } from "@phosphor-icons/react";
import { InlineMessage } from "../components/InlineMessage";
import { useAppTitle } from "../lib/adminTitle";
import { apiGet, apiPost, ApiError } from "../lib/api";
import { clearToken, setMustChangePassword } from "../lib/auth";

interface CurrentUser {
  username: string;
  must_change_password: boolean;
}

export function ForceChangePasswordPage() {
  const { t } = useTranslation();
  const appTitle = useAppTitle();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [logoutLoading, setLogoutLoading] = useState(false);

  const user = useQuery({
    queryKey: ["current-user"],
    queryFn: () => apiGet<CurrentUser>("/api/admin/user/current"),
  });

  useEffect(() => {
    if (user.data) {
      const required = Boolean(user.data.must_change_password);
      setMustChangePassword(required);
      if (!required) {
        navigate("/users", { replace: true });
      }
    }
  }, [navigate, user.data]);

  const finishSession = (message?: string) => {
    setMustChangePassword(false);
    clearToken();
    queryClient.clear();
    navigate("/login", {
      replace: true,
      state: message ? { message } : undefined,
    });
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    if (newPassword.length < 4) {
      setError(t("passwordMinLength"));
      return;
    }
    if (newPassword !== confirmPassword) {
      setError(t("passwordMismatch"));
      return;
    }
    setLoading(true);
    try {
      await apiPost("/api/admin/user/changeCurPwd", {
        old_password: oldPassword,
        new_password: newPassword,
      });
      finishSession(t("passwordUpdatedLoginAgain"));
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  const logout = async () => {
    setLogoutLoading(true);
    try {
      await apiPost("/api/admin/logout");
    } catch {
      /* local cleanup still applies */
    } finally {
      finishSession();
      setLogoutLoading(false);
    }
  };

  return (
    <div className="relative min-h-full overflow-auto bg-kumo-base px-4 py-6 text-kumo-default sm:px-6 lg:px-8">
      <div className="login-grid-bg pointer-events-none absolute inset-0 opacity-60" />
      <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-kumo-brand/70" />
      <div className="relative z-10 mx-auto grid min-h-[calc(100dvh-3rem)] w-full max-w-5xl items-center">
        <div className="grid overflow-hidden rounded-lg border border-kumo-line bg-kumo-elevated shadow-lg lg:grid-cols-[minmax(0,1fr)_420px]">
          <section className="relative overflow-hidden p-6 sm:p-8 lg:p-10">
            <div className="flex h-full min-h-[220px] flex-col justify-between gap-8">
              <div>
                <div className="inline-flex min-h-9 items-center gap-2 rounded-lg border border-kumo-line bg-kumo-base px-3 text-xs font-semibold uppercase text-kumo-subtle">
                  <ShieldCheck size={16} aria-hidden />
                  <span>{appTitle}</span>
                </div>
                <h1 className="mt-6 max-w-[16ch] break-words text-3xl font-semibold leading-tight sm:text-4xl">
                  {t("changePasswordRequiredTitle")}
                </h1>
                <p className="mt-4 max-w-md text-sm leading-6 text-kumo-subtle sm:text-base">
                  {t("changePasswordRequiredSubtitle")}
                </p>
              </div>

              <dl className="grid max-w-md gap-3 border-t border-kumo-line pt-4 text-sm">
                <div className="flex items-center justify-between gap-4">
                  <dt className="text-kumo-subtle">{t("signedInAs")}</dt>
                  <dd className="min-w-0 truncate font-medium">
                    {user.data?.username || t("loading")}
                  </dd>
                </div>
                <div className="flex items-center justify-between gap-4">
                  <dt className="text-kumo-subtle">{t("status")}</dt>
                  <dd className="font-medium">{t("passwordChangeRequired")}</dd>
                </div>
              </dl>
            </div>
          </section>

          <form
            onSubmit={submit}
            className="border-t border-kumo-line bg-kumo-base p-6 sm:p-8 lg:border-l lg:border-t-0 lg:p-10"
          >
            <div className="mb-8">
              <div className="flex min-h-6 items-center gap-2 text-xs font-semibold uppercase text-kumo-subtle">
                <Key size={16} aria-hidden />
                <span>{t("adminAccess")}</span>
              </div>
              <h2 className="mt-3 text-2xl font-semibold">
                {t("changePassword")}
              </h2>
              <p className="mt-2 text-sm text-kumo-subtle">
                {t("changePasswordFormSubtitle")}
              </p>
            </div>

            <div className="space-y-4">
              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("oldPassword")}
                </span>
                <Input
                  aria-label={t("oldPassword")}
                  type="password"
                  value={oldPassword}
                  autoComplete="current-password"
                  autoFocus
                  className="w-full"
                  onChange={(e) => setOldPassword(e.target.value)}
                />
              </label>
              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("newPassword")}
                </span>
                <Input
                  aria-label={t("newPassword")}
                  type="password"
                  value={newPassword}
                  autoComplete="new-password"
                  className="w-full"
                  onChange={(e) => setNewPassword(e.target.value)}
                />
              </label>
              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("confirmPassword")}
                </span>
                <Input
                  aria-label={t("confirmPassword")}
                  type="password"
                  value={confirmPassword}
                  autoComplete="new-password"
                  className="w-full"
                  onChange={(e) => setConfirmPassword(e.target.value)}
                />
              </label>
              {error && <InlineMessage tone="error">{error}</InlineMessage>}
              <Button
                type="submit"
                className="w-full justify-center"
                disabled={loading || logoutLoading}
                loading={loading}
              >
                {t("save")}
              </Button>
              <Button
                type="button"
                variant="secondary"
                className="w-full justify-center"
                disabled={loading || logoutLoading}
                loading={logoutLoading}
                onClick={() => void logout()}
              >
                <SignOut size={18} aria-hidden />
                {t("logout")}
              </Button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}
