import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { InlineMessage } from "../../components/InlineMessage";
import { useAppBranding } from "../../lib/adminTitle";

export function WebGuestPage() {
  const { t } = useTranslation();
  const { title, logoUrl } = useAppBranding();
  const [params, setParams] = useSearchParams();
  const [guest] = useState(() => ({
    token: params.get("token") ?? "",
    peer_id: params.get("peer") ?? "",
  }));
  const [error, setError] = useState(false);
  const valid = /^[A-Za-z0-9_-]{43}$/.test(guest.token) && guest.peer_id.length > 0 && guest.peer_id.length <= 128;

  useEffect(() => {
    if (params.has("token")) setParams({}, { replace: true });
  }, [params, setParams]);

  function open() {
    try {
      sessionStorage.setItem("rustdesk-console.guest", JSON.stringify(guest));
      window.location.assign(`/webguest/#/${encodeURIComponent(guest.peer_id)}`);
    } catch {
      setError(true);
    }
  }

  return (
    <main className="grid min-h-dvh place-items-center bg-kumo-base p-4 text-kumo-default">
      <section className="w-full max-w-md rounded-xl border border-kumo-line bg-kumo-elevated p-6 sm:p-8" aria-labelledby="guest-title">
        <div className="flex items-center gap-2 text-sm font-medium text-kumo-subtle">
          <img src={logoUrl} alt="" className="size-6 object-contain" />
          {title}
        </div>
        <h1 id="guest-title" className="mt-4 text-2xl font-semibold">{t("webGuestTitle")}</h1>
        {valid ? (
          <>
            <p className="mt-2 break-words text-sm font-medium">{t("webGuestDevice", { id: guest.peer_id })}</p>
            <p className="mt-2 text-sm leading-6 text-kumo-subtle">{t("webGuestHint")}</p>
            {error && <InlineMessage className="mt-4" tone="error">{t("webGuestLaunchFailed")}</InlineMessage>}
            <Button className="mt-5" variant="primary" onClick={open}>{t("webGuestConnect")}</Button>
          </>
        ) : (
          <InlineMessage className="mt-4" tone="error">{t("webGuestInvalid")}</InlineMessage>
        )}
      </section>
    </main>
  );
}
