import { useState, type FormEvent } from "react";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import * as QRCode from "qrcode";
import { InlineMessage } from "../components/InlineMessage";
import { apiPost } from "../lib/api";
import { clearToken } from "../lib/auth";

interface Enrollment {
  secret: string;
  qr: string;
}

export function EnrollTotpPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const cache = useQueryClient();
  const [enrollment, setEnrollment] = useState<Enrollment>();
  const [code, setCode] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const finish = (message?: string) => {
    clearToken();
    cache.clear();
    navigate("/login", { replace: true, state: message ? { message } : undefined });
  };

  const start = async () => {
    setBusy(true);
    setError("");
    try {
      const { secret, uri } = await apiPost<{ secret: string; uri: string }>(
        "/api/admin/user/myTfaSetup",
      );
      const qr = await QRCode.toDataURL(uri, { width: 176 }).catch(() => "");
      setEnrollment({ secret, qr });
    } catch (err) {
      setError(err instanceof Error ? err.message : t("operationFailed"));
    } finally {
      setBusy(false);
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!enrollment || !/^\d{6}$/.test(code)) return;
    setBusy(true);
    setError("");
    try {
      await apiPost("/api/admin/user/myTfaEnable", { secret: enrollment.secret, code });
      finish(t("totpEnabledLoginAgain"));
    } catch (err) {
      setError(err instanceof Error ? err.message : t("operationFailed"));
    } finally {
      setBusy(false);
    }
  };

  const signOut = async () => {
    setBusy(true);
    try {
      await apiPost("/api/admin/logout");
    } catch {
      // Clear local state even if the enrollment session already expired.
    } finally {
      finish();
    }
  };

  return (
    <section className="min-h-full bg-kumo-base px-4 py-12 text-kumo-default">
      <div className="mx-auto max-w-md space-y-4 rounded-lg border border-kumo-line bg-kumo-elevated p-5 sm:p-7">
        <h1 className="text-xl font-semibold">{t("totpEnrollmentRequired")}</h1>
        <p className="text-sm leading-6 text-kumo-subtle">{t("totpSetupHint")}</p>
        {error && <InlineMessage tone="error">{error}</InlineMessage>}
        {enrollment ? (
          <form className="space-y-4" onSubmit={(event) => void submit(event)}>
            {enrollment.qr && (
              <img src={enrollment.qr} alt={t("totpQrCode")} width={176} height={176} />
            )}
            <div>
              <p className="text-sm font-medium">{t("totpSecret")}</p>
              <code className="break-all text-sm">{enrollment.secret}</code>
            </div>
            <Input
              label={t("verificationCode")}
              value={code}
              inputMode="numeric"
              autoComplete="one-time-code"
              maxLength={6}
              onChange={(event) => setCode(event.target.value)}
            />
            <Button type="submit" loading={busy} disabled={!/^\d{6}$/.test(code)}>
              {t("enableTotp")}
            </Button>
          </form>
        ) : (
          <Button loading={busy} onClick={() => void start()}>{t("setupTotp")}</Button>
        )}
        <Button variant="secondary" disabled={busy} onClick={() => void signOut()}>
          {t("logout")}
        </Button>
      </div>
    </section>
  );
}
