import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  resourceFormDialogPanelClass,
} from "./DialogLayout";
import { ApiError, apiPost } from "../lib/api";

interface ProviderTestResult {
  op: string;
  oauth_type: string;
  ready: boolean;
  redirect_uri: string;
  auth_url: string;
  token_url: string;
  userinfo_url: string;
  jwks_uri: string;
  scopes: string[];
}

export function OAuthProviderActions({
  provider,
}: {
  provider: Record<string, unknown>;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ProviderTestResult | null>(null);

  const run = async () => {
    setOpen(true);
    setLoading(true);
    setError("");
    setResult(null);
    try {
      const res = await apiPost<ProviderTestResult>(
        "/api/admin/oauth/test",
        provider,
      );
      setResult(res);
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : t("operationFailed"),
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <>
      <Button size="sm" variant="secondary" onClick={() => void run()}>
        {t("testProvider")}
      </Button>
      {open && (
        <div
          aria-hidden="true"
          className="pointer-events-none fixed inset-0 z-40 bg-kumo-recessed opacity-95"
        />
      )}
      <Dialog.Root open={open} onOpenChange={setOpen}>
        <Dialog size="lg" className={resourceFormDialogPanelClass}>
          <DialogHeader
            title={t("oauthProviderTest")}
            description={t("oauthProviderTestHint")}
          />
          <DialogBody>
            <div className="space-y-3">
              {loading && (
                <div className="text-sm text-kumo-subtle">{t("checking")}</div>
              )}
              {error && (
                <div className="rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm text-kumo-danger">
                  {error}
                </div>
              )}
              {result && (
                <div className="space-y-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge>{result.op}</Badge>
                    <Badge>{result.oauth_type}</Badge>
                    <Badge>{result.ready ? t("available") : t("notAvailable")}</Badge>
                  </div>
                  <dl className="grid gap-2 text-sm">
                    <Detail label={t("redirectUri")} value={result.redirect_uri} />
                    <Detail label={t("authEndpoint")} value={result.auth_url} />
                    <Detail label={t("tokenEndpoint")} value={result.token_url} />
                    <Detail label={t("userinfoEndpoint")} value={result.userinfo_url} />
                    <Detail label={t("jwksUri")} value={result.jwks_uri} />
                    <Detail label={t("scopes")} value={result.scopes.join(" ")} />
                  </dl>
                </div>
              )}
            </div>
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={() => setOpen(false)}>
              {t("close")}
            </Button>
          </DialogFooter>
        </Dialog>
      </Dialog.Root>
    </>
  );
}

function Detail({ label, value }: { label: string; value: string }) {
  if (!value) return null;
  return (
    <div className="grid gap-1 rounded-md border border-kumo-line bg-kumo-base px-3 py-2">
      <dt className="text-xs text-kumo-subtle">{label}</dt>
      <dd className="break-all font-mono text-xs text-kumo-default">{value}</dd>
    </div>
  );
}
