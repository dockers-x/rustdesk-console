import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { useAppBranding } from "../../lib/adminTitle";
import { copyToClipboard } from "../../lib/clipboard";
import { InlineMessage } from "../../components/InlineMessage";

interface ClientSetup {
  encoded_config: string;
  mobile_config_uri: string;
  config_command: Record<"windows" | "macos" | "linux", string[]>;
  download_path: string;
}

export function EnrollmentPage() {
  const { t } = useTranslation();
  const { title, logoUrl } = useAppBranding();
  const [params, setParams] = useSearchParams();
  const [code, setCode] = useState(() => (params.get("code") || "").slice(0, 32));
  const [setup, setSetup] = useState<ClientSetup>();
  const [status, setStatus] = useState<"idle" | "loading" | "error">("idle");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (params.has("code")) setParams({}, { replace: true });
  }, [params, setParams]);

  async function redeem() {
    setStatus("loading");
    try {
      const response = await fetch("/api/enrollment-invitations/redeem", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code }),
        credentials: "omit",
        cache: "no-store",
      });
      if (!response.ok) throw new Error("Enrollment request failed");
      const result = await response.json() as { code: number; data?: ClientSetup };
      if (result.code !== 0 || !result.data) throw new Error("Invalid enrollment code");
      setSetup(result.data);
      setCode("");
      setStatus("idle");
    } catch {
      setStatus("error");
    }
  }

  return (
    <main className="grid min-h-dvh place-items-center bg-kumo-base p-4 text-kumo-default">
      <section className="w-full max-w-xl rounded-xl border border-kumo-line bg-kumo-elevated p-6 sm:p-8" aria-labelledby="enrollment-heading">
        <div className="flex items-center gap-2 text-sm font-medium text-kumo-subtle">
          <img src={logoUrl} alt="" className="size-6 object-contain" />
          {title}
        </div>
        <h1 id="enrollment-heading" className="mt-4 text-2xl font-semibold">{t("enrollmentTitle")}</h1>
        {setup ? (
          <div className="mt-4 space-y-4">
            <div className="flex flex-wrap gap-2">
              {setup.download_path && <LinkButton href={setup.download_path} variant="primary">{t("enrollmentDownload")}</LinkButton>}
              <LinkButton href={setup.mobile_config_uri} variant={setup.download_path ? "secondary" : "primary"}>
                {t("enrollmentOpenRustDesk")}
              </LinkButton>
              <Button variant="secondary" onClick={() => { void copyToClipboard(setup.encoded_config).then(setCopied); }}>
                {t(copied ? "copied" : "enrollmentCopyConfig")}
              </Button>
            </div>
            <p className="text-sm leading-6 text-kumo-subtle">{t("enrollmentApprovalHint")}</p>
            <details className="rounded-lg border border-kumo-line p-3">
              <summary className="cursor-pointer text-sm font-medium">{t("deploymentCommands")}</summary>
              <p className="mt-2 text-sm leading-6 text-kumo-subtle">{t("enrollmentDesktopHint")}</p>
              {(["windows", "macos", "linux"] as const).map((platform) => (
                <div className="mt-3" key={platform}>
                  <h2 className="mb-1 text-sm font-medium">{t(platform)}</h2>
                  <pre className="overflow-x-auto rounded-md bg-kumo-recessed p-2 text-xs">
                    {setup.config_command[platform].join("\n")}
                  </pre>
                </div>
              ))}
            </details>
          </div>
        ) : (
          <form className="mt-3 space-y-4" onSubmit={(event) => {
            event.preventDefault();
            if (code.trim() && status !== "loading") void redeem();
          }}>
            <p className="text-sm leading-6 text-kumo-subtle">{t("enrollmentRedeemHint")}</p>
            <label className="block text-sm font-medium">
              <span className="mb-1.5 block">{t("enrollmentCode")}</span>
              <Input value={code} maxLength={32} autoCapitalize="characters" autoComplete="off"
                spellCheck={false} onChange={(event) => {
                  setCode(event.target.value);
                  if (status === "error") setStatus("idle");
                }} />
            </label>
            {status === "error" && <InlineMessage tone="error">{t("enrollmentRedeemFailed")}</InlineMessage>}
            <Button type="submit" variant="primary" loading={status === "loading"} disabled={status === "loading" || !code.trim()}>
              {t("enrollmentRedeem")}
            </Button>
          </form>
        )}
      </section>
    </main>
  );
}
