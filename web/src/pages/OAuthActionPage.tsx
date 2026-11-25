import { useState } from "react";
import { useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet, apiPost, ApiError } from "../lib/api";

interface OAuthInfo {
  id?: string;
  op?: string;
  device_name?: string;
  device_os?: string;
  device_type?: string;
}

export function OAuthActionPage() {
  const { t } = useTranslation();
  const { code = "" } = useParams();
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const { data, isLoading } = useQuery({
    queryKey: ["oauth-info", code],
    enabled: code.length > 0,
    queryFn: () => apiGet<OAuthInfo>("/api/admin/oauth/info", { code }),
  });

  const submit = async () => {
    setError("");
    setLoading(true);
    try {
      await apiPost("/api/admin/oauth/confirm", { code });
      setMessage(t("operationSuccess"));
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  const rows = [
    [t("device"), data?.device_name],
    [t("deviceId"), data?.id],
    [t("os"), data?.device_os],
  ];

  return (
    <div className="mx-auto max-w-2xl">
      <div className="mb-4">
        <h1 className="text-2xl font-semibold">
          {t("oauthLogining")}
        </h1>
        <p className="mt-1 text-sm text-kumo-subtle">{t("oauthCloseNote")}</p>
      </div>
      <div className="admin-surface rounded-xl border border-kumo-line p-5">
        {isLoading ? (
          <p className="text-sm text-kumo-subtle">{t("loading")}</p>
        ) : (
          <dl className="grid gap-3 text-sm sm:grid-cols-[140px_1fr]">
            {rows.map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-kumo-subtle">{label}</dt>
                <dd className="min-w-0 break-words font-medium">
                  {value || "—"}
                </dd>
              </div>
            ))}
          </dl>
        )}
        {message && (
          <InlineMessage tone="success" className="mt-4">
            {message}
          </InlineMessage>
        )}
        {error && (
          <InlineMessage tone="error" className="mt-4">
            {error}
          </InlineMessage>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <Button variant="secondary" onClick={() => window.close()}>
            {t("close")}
          </Button>
          <Button variant="primary" onClick={submit} disabled={loading || !code || Boolean(message)}>
            {t("confirmOauth")}
          </Button>
        </div>
      </div>
    </div>
  );
}
