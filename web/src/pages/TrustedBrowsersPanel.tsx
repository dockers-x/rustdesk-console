import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Checkbox } from "@cloudflare/kumo/components/checkbox";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPost } from "../lib/api";
import { formatUnixSeconds } from "../lib/dateFormat";

interface TrustedBrowser {
  id: string;
  browser: string;
  platform: string;
  created_at: number;
  last_used_at: number;
  expires_at: number;
  current: boolean;
}

const path = "/api/admin/login/trusted-browsers";
const queryKey = [path];

export function TrustedBrowsersPanel() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const browsers = useQuery({ queryKey, queryFn: () => apiGet<TrustedBrowser[]>(path) });
  const [target, setTarget] = useState<TrustedBrowser | "all" | null>(null);
  const [endSessions, setEndSessions] = useState(true);
  const revoke = useMutation({
    mutationFn: (selection: TrustedBrowser | "all") =>
      apiPost(selection === "all" ? `${path}/revoke-all` : `${path}/${selection.id}/revoke`, {
        end_sessions: endSessions,
      }),
    onSuccess: () => {
      setTarget(null);
      void client.invalidateQueries({ queryKey });
    },
  });

  const askToRevoke = (selection: TrustedBrowser | "all") => {
    revoke.reset();
    setEndSessions(true);
    setTarget(selection);
  };

  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-base p-4">
      <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">{t("trustedBrowsers")}</h3>
          <p className="mt-1 text-sm leading-6 text-kumo-subtle">{t("trustedBrowsersHint")}</p>
        </div>
        {!!browsers.data?.length && (
          <Button size="sm" variant="secondary-destructive" onClick={() => askToRevoke("all")}>
            {t("revokeAllBrowsers")}
          </Button>
        )}
      </div>
      {browsers.isPending && <TableState tone="loading">{t("loading")}</TableState>}
      {browsers.isError && (
        <TableState tone="error">
          <p>{browsers.error.message}</p>
          <Button size="sm" variant="secondary" onClick={() => void browsers.refetch()}>{t("retryLoad")}</Button>
        </TableState>
      )}
      {browsers.data?.length === 0 && <TableState tone="empty">{t("trustedBrowsersEmpty")}</TableState>}
      {!!browsers.data?.length && (
        <ul className="divide-y divide-kumo-line rounded-lg border border-kumo-line bg-kumo-elevated">
          {browsers.data.map((browser) => (
            <li key={browser.id} className="flex flex-col gap-3 p-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
                  {[browser.browser || t("unknownBrowser"), browser.platform].filter(Boolean).join(" · ")}
                  {browser.current && <Badge variant="info">{t("currentBrowser")}</Badge>}
                </div>
                <dl className="mt-2 flex flex-wrap gap-x-5 gap-y-2 text-xs text-kumo-subtle">
                  <div><dt>{t("browserTrustedAt")}</dt><dd>{formatUnixSeconds(browser.created_at)}</dd></div>
                  <div><dt>{t("browserLastUsedAt")}</dt><dd>{formatUnixSeconds(browser.last_used_at)}</dd></div>
                  <div><dt>{t("browserExpiresAt")}</dt><dd>{formatUnixSeconds(browser.expires_at)}</dd></div>
                </dl>
              </div>
              <Button size="sm" variant="secondary-destructive" onClick={() => askToRevoke(browser)}>
                {t("revokeBrowser")}
              </Button>
            </li>
          ))}
        </ul>
      )}

      {target && (
        <Dialog.Root open onOpenChange={(open) => { if (!open && !revoke.isPending) setTarget(null); }}>
          <Dialog className={dialogPanelClass}>
            <DialogHeader
              title={t(target === "all" ? "revokeAllBrowsers" : "revokeBrowser")}
              description={t("revokeBrowserHint")}
            />
            <DialogBody>
              <Checkbox
                label={t("endBrowserSessions")}
                checked={endSessions}
                disabled={revoke.isPending}
                onCheckedChange={setEndSessions}
              />
              {(target === "all" || target.current) && endSessions && (
                <p className="mt-2 text-sm text-kumo-subtle">{t("endCurrentBrowserSessionHint")}</p>
              )}
              {revoke.isError && <InlineMessage tone="error">{revoke.error.message}</InlineMessage>}
            </DialogBody>
            <DialogFooter>
              <Button variant="secondary" disabled={revoke.isPending} onClick={() => setTarget(null)}>{t("cancel")}</Button>
              <Button variant="destructive" loading={revoke.isPending} onClick={() => revoke.mutate(target)}>{t("revokeBrowser")}</Button>
            </DialogFooter>
          </Dialog>
        </Dialog.Root>
      )}
    </section>
  );
}
