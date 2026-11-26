import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import * as QRCode from "qrcode";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { InlineMessage } from "../../components/InlineMessage";
import { apiPost, http } from "../../lib/api";
import { copyToClipboard } from "../../lib/clipboard";
import { formatUnixSeconds } from "../../lib/dateFormat";

interface Invitation {
  id: number;
  code: string;
  expires_at: number;
}

const invitationApi = "/api/admin/enrollment-invitations";

function portalPath(text: string) {
  if (!text.trim()) return "";
  try {
    const url = new URL(text, window.location.origin);
    if (url.origin === window.location.origin && /^\/client-downloads\/[A-Za-z0-9_-]{43}$/.test(url.pathname)) {
      return url.pathname;
    }
  } catch {
    // The field displays a localized validation message.
  }
  return null;
}

export function EnrollmentPanel() {
  const { t } = useTranslation();
  const [portal, setPortal] = useState("");
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [invitation, setInvitation] = useState<(Invitation & { link: string; qr?: string })>();
  const create = useMutation({
    mutationFn: async (downloadPath: string) => {
      const created = await apiPost<Invitation>(invitationApi, { download_path: downloadPath });
      const url = new URL(window.location.pathname, window.location.origin);
      url.hash = `/pair-device?code=${created.code}`;
      const link = url.toString();
      const qr = await QRCode.toDataURL(link, { width: 160, margin: 2 }).catch(() => undefined);
      return { ...created, link, qr };
    },
    onSuccess: setInvitation,
  });
  const revoke = useMutation({
    mutationFn: (id: number) => http.delete(`${invitationApi}/${id}`),
    onSuccess: () => setInvitation(undefined),
  });

  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
      <h2 className="text-base font-semibold">{t("enrollmentTitle")}</h2>
      <p className="mt-1 max-w-2xl text-sm leading-6 text-kumo-subtle">{t("enrollmentAdminHint")}</p>
      {!invitation ? (
        <form className="mt-4 flex flex-wrap items-end gap-3" onSubmit={(event) => {
          event.preventDefault();
          const path = portalPath(portal);
          if (path === null) {
            setError(t("enrollmentInvalidDownload"));
            return;
          }
          setError("");
          revoke.reset();
          setCopied(false);
          create.mutate(path);
        }}>
          <label className="block w-full max-w-md text-sm">
            <span className="mb-1.5 block font-medium">{t("enrollmentOptionalDownload")}</span>
            <Input type="text" value={portal} maxLength={512} disabled={create.isPending} onChange={(event) => {
              setPortal(event.target.value);
              setError("");
              create.reset();
            }} />
          </label>
          <Button type="submit" variant="primary" loading={create.isPending} disabled={create.isPending}>
            {t("enrollmentCreate")}
          </Button>
        </form>
      ) : (
        <div className="mt-4 flex flex-wrap items-start gap-4">
          {invitation.qr && (
            <img src={invitation.qr} alt={t("enrollmentLink")} width={160} height={160} className="rounded-md border border-kumo-line" />
          )}
          <div className="min-w-0">
            <div className="text-sm text-kumo-subtle">{t("enrollmentCode")}</div>
            <code className="mt-1 block text-lg font-semibold tabular-nums tracking-wider">
              {invitation.code.match(/.{4}/g)?.join("-")}
            </code>
            <p className="mt-1 text-sm text-kumo-subtle">
              {t("enrollmentCodeExpires", { time: formatUnixSeconds(invitation.expires_at) })}
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button variant="secondary" onClick={() => {
                void copyToClipboard(invitation.link).then(setCopied);
              }}>{t(copied ? "copied" : "enrollmentCopyLink")}</Button>
              <Button variant="ghost" loading={revoke.isPending} disabled={revoke.isPending} onClick={() => revoke.mutate(invitation.id)}>
                {t("enrollmentRevoke")}
              </Button>
            </div>
          </div>
        </div>
      )}
      {(error || create.error || revoke.error) && (
        <InlineMessage tone="error" className="mt-3">
          {error || create.error?.message || revoke.error?.message}
        </InlineMessage>
      )}
    </section>
  );
}
