import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { apiGet, apiPost, ApiError } from "../lib/api";

interface CurrentUser {
  username: string;
  email: string;
  nickname: string;
}

interface AdminConfig {
  title?: string;
  hello?: string;
}

interface OAuthStatus {
  op: string;
  status: number;
}

interface BindStart {
  code: string;
  url: string;
}

export function MyProfilePage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [pwdMessage, setPwdMessage] = useState("");
  const [pwdError, setPwdError] = useState("");

  const user = useQuery({
    queryKey: ["current-user"],
    queryFn: () => apiGet<CurrentUser>("/api/admin/user/current"),
  });
  const config = useQuery({
    queryKey: ["admin-config"],
    queryFn: () => apiGet<AdminConfig>("/api/admin/config/admin"),
  });
  const oauth = useQuery({
    queryKey: ["my-oauth"],
    queryFn: () => apiPost<OAuthStatus[]>("/api/admin/user/myOauth"),
  });

  const changePwd = useMutation({
    mutationFn: () =>
      apiPost("/api/admin/user/changeCurPwd", {
        old_password: oldPassword,
        new_password: newPassword,
      }),
    onSuccess: () => {
      setOldPassword("");
      setNewPassword("");
      setConfirmPassword("");
      setPwdMessage(t("operationSuccess"));
    },
    onError: (err) => {
      const ae = err as ApiError;
      setPwdError(ae.message || t("operationFailed"));
    },
  });

  const bindOauth = async (op: string) => {
    const res = await apiPost<BindStart>("/api/admin/oauth/bind", { op });
    window.open(res.url, "_blank", "noopener,noreferrer");
  };

  const unbindOauth = async (op: string) => {
    if (!confirm(t("confirmUnbind"))) return;
    await apiPost("/api/admin/oauth/unbind", { op });
    void qc.invalidateQueries({ queryKey: ["my-oauth"] });
  };

  const submitPassword = (e: React.FormEvent) => {
    e.preventDefault();
    setPwdMessage("");
    setPwdError("");
    if (newPassword !== confirmPassword) {
      setPwdError(t("passwordMismatch"));
      return;
    }
    changePwd.mutate();
  };

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-2xl font-semibold">{t("myInfo")}</h1>
        <p className="mt-1 text-sm text-color-muted">{t("myInfoHint")}</p>
      </div>

      <section className="rounded-lg border border-color-border bg-kumo-elevated p-5">
        <h2 className="mb-4 text-base font-semibold">{t("account")}</h2>
        <dl className="grid gap-3 text-sm sm:grid-cols-[140px_1fr]">
          <div className="contents">
            <dt className="text-color-muted">{t("username")}</dt>
            <dd className="font-medium">{user.data?.username || "—"}</dd>
          </div>
          <div className="contents">
            <dt className="text-color-muted">{t("email")}</dt>
            <dd className="font-medium">{user.data?.email || "—"}</dd>
          </div>
          <div className="contents">
            <dt className="text-color-muted">{t("nickname")}</dt>
            <dd className="font-medium">{user.data?.nickname || "—"}</dd>
          </div>
        </dl>
      </section>

      <section className="rounded-lg border border-color-border bg-kumo-elevated p-5">
        <h2 className="mb-4 text-base font-semibold">{t("changePassword")}</h2>
        <form onSubmit={submitPassword} className="grid max-w-xl gap-3">
          <label className="block">
            <span className="mb-1 block text-sm">{t("oldPassword")}</span>
            <Input
              type="password"
              value={oldPassword}
              autoComplete="current-password"
              onChange={(e) => setOldPassword(e.target.value)}
            />
          </label>
          <label className="block">
            <span className="mb-1 block text-sm">{t("newPassword")}</span>
            <Input
              type="password"
              value={newPassword}
              autoComplete="new-password"
              onChange={(e) => setNewPassword(e.target.value)}
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
          {pwdMessage && <p className="text-sm text-green-600">{pwdMessage}</p>}
          {pwdError && <p className="text-sm text-red-500">{pwdError}</p>}
          <div>
            <Button type="submit" disabled={changePwd.isPending}>
              {t("save")}
            </Button>
          </div>
        </form>
      </section>

      <section className="rounded-lg border border-color-border bg-kumo-elevated p-5">
        <h2 className="mb-4 text-base font-semibold">{t("oauthBinding")}</h2>
        <div className="overflow-x-auto rounded-lg border border-color-border">
          <Table>
            <Table.Header>
              <Table.Row>
                <Table.Head>{t("op")}</Table.Head>
                <Table.Head>{t("status")}</Table.Head>
                <Table.Head>{t("actions")}</Table.Head>
              </Table.Row>
            </Table.Header>
            <Table.Body>
              {(oauth.data ?? []).map((row) => (
                <Table.Row key={row.op}>
                  <Table.Cell>{row.op}</Table.Cell>
                  <Table.Cell>
                    {row.status === 1 ? (
                      <Badge>{t("hasBind")}</Badge>
                    ) : (
                      t("noBind")
                    )}
                  </Table.Cell>
                  <Table.Cell>
                    {row.status === 1 ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void unbindOauth(row.op)}
                      >
                        {t("unbind")}
                      </Button>
                    ) : (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void bindOauth(row.op)}
                      >
                        {t("toBind")}
                      </Button>
                    )}
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
          {!oauth.isLoading && (oauth.data ?? []).length === 0 && (
            <div className="p-4 text-sm text-color-muted">{t("noData")}</div>
          )}
        </div>
      </section>

      {config.data?.hello && (
        <section className="rounded-lg border border-color-border bg-kumo-elevated p-5">
          <h2 className="mb-3 text-base font-semibold">
            {config.data.title || t("notice")}
          </h2>
          <div className="whitespace-pre-wrap text-sm leading-6">
            {config.data.hello}
          </div>
        </section>
      )}
    </div>
  );
}
