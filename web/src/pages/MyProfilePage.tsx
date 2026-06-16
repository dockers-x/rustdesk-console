import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPost, ApiError } from "../lib/api";
import { clearToken } from "../lib/auth";

interface CurrentUser {
  username: string;
  email: string;
  nickname: string;
  must_change_password: boolean;
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
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [pwdMessage, setPwdMessage] = useState("");
  const [pwdError, setPwdError] = useState("");
  const [unbindTarget, setUnbindTarget] = useState("");

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
      setPwdMessage(t("passwordUpdatedLoginAgain"));
      clearToken();
      qc.clear();
      navigate("/login", {
        replace: true,
        state: { message: t("passwordUpdatedLoginAgain") },
      });
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

  const unbind = useMutation({
    mutationFn: (op: string) => apiPost("/api/admin/oauth/unbind", { op }),
    onSuccess: () => {
      setUnbindTarget("");
      void qc.invalidateQueries({ queryKey: ["my-oauth"] });
    },
  });

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
        <p className="mt-1 text-sm text-kumo-subtle">{t("myInfoHint")}</p>
      </div>

      <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
        <h2 className="mb-4 text-base font-semibold">{t("account")}</h2>
        <dl className="grid gap-3 text-sm sm:grid-cols-[140px_1fr]">
          <div className="contents">
            <dt className="text-kumo-subtle">{t("username")}</dt>
            <dd className="font-medium">{user.data?.username || "—"}</dd>
          </div>
          <div className="contents">
            <dt className="text-kumo-subtle">{t("email")}</dt>
            <dd className="font-medium">{user.data?.email || "—"}</dd>
          </div>
          <div className="contents">
            <dt className="text-kumo-subtle">{t("nickname")}</dt>
            <dd className="font-medium">{user.data?.nickname || "—"}</dd>
          </div>
        </dl>
      </section>

      <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
        <h2 className="mb-4 text-base font-semibold">{t("changePassword")}</h2>
        <form onSubmit={submitPassword} className="grid max-w-xl gap-3">
          <label className="block">
            <span className="mb-1 block text-sm">{t("oldPassword")}</span>
            <Input
              aria-label={t("oldPassword")}
              type="password"
              value={oldPassword}
              autoComplete="current-password"
              onChange={(e) => setOldPassword(e.target.value)}
            />
          </label>
          <label className="block">
            <span className="mb-1 block text-sm">{t("newPassword")}</span>
            <Input
              aria-label={t("newPassword")}
              type="password"
              value={newPassword}
              autoComplete="new-password"
              onChange={(e) => setNewPassword(e.target.value)}
            />
          </label>
          <label className="block">
            <span className="mb-1 block text-sm">{t("confirmPassword")}</span>
            <Input
              aria-label={t("confirmPassword")}
              type="password"
              value={confirmPassword}
              autoComplete="new-password"
              onChange={(e) => setConfirmPassword(e.target.value)}
            />
          </label>
          {pwdMessage && (
            <InlineMessage tone="success">{pwdMessage}</InlineMessage>
          )}
          {pwdError && <InlineMessage tone="error">{pwdError}</InlineMessage>}
          <div>
            <Button
              type="submit"
              disabled={changePwd.isPending}
              loading={changePwd.isPending}
            >
              {t("save")}
            </Button>
          </div>
        </form>
      </section>

      <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
        <h2 className="mb-4 text-base font-semibold">{t("oauthBinding")}</h2>
        <div className="overflow-x-auto rounded-lg border border-kumo-line">
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
                        variant="secondary-destructive"
                        onClick={() => {
                          unbind.reset();
                          setUnbindTarget(row.op);
                        }}
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
          {oauth.isLoading && (
            <TableState tone="loading">{t("loading")}</TableState>
          )}
          {oauth.error && (
            <TableState tone="error">
              {(oauth.error as Error).message || t("operationFailed")}
            </TableState>
          )}
          {!oauth.isLoading && !oauth.error && (oauth.data ?? []).length === 0 && (
            <TableState tone="empty">{t("noData")}</TableState>
          )}
        </div>
      </section>

      {config.data?.hello && (
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <h2 className="mb-3 text-base font-semibold">
            {config.data.title || t("notice")}
          </h2>
          <div className="whitespace-pre-wrap text-sm leading-6">
            {config.data.hello}
          </div>
        </section>
      )}

      <ConfirmDialog
        open={Boolean(unbindTarget)}
        title={t("confirmUnbindTitle")}
        description={t("confirmUnbindDescription")}
        confirmLabel={t("unbind")}
        cancelLabel={t("cancel")}
        error={
          unbind.error
            ? (unbind.error as Error).message || t("operationFailed")
            : undefined
        }
        loading={unbind.isPending}
        onOpenChange={(next) => {
          if (!next) {
            setUnbindTarget("");
            unbind.reset();
          }
        }}
        onConfirm={() => {
          if (unbindTarget) unbind.mutate(unbindTarget);
        }}
      />
    </div>
  );
}
