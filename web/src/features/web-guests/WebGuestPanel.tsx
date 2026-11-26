import { useState } from "react";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { InlineMessage } from "../../components/InlineMessage";
import { apiGet, apiPost, http } from "../../lib/api";
import { copyToClipboard } from "../../lib/clipboard";
import { formatUnixSeconds } from "../../lib/dateFormat";

interface Grant {
  id: number;
  peer_id: string;
  expires_at: number;
  used_count: number;
  max_uses: number;
}

const api = "/api/admin/web-guests";
const queryKey = ["web-guest-grants"];
const durations = [15, 30, 60, 240, 1440];

export function WebGuestPanel() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [peerId, setPeerId] = useState("");
  const [page, setPage] = useState(1);
  const [duration, setDuration] = useState(60);
  const [maxUses, setMaxUses] = useState(1);
  const [newLink, setNewLink] = useState<{ id: number; url: string }>();
  const [copied, setCopied] = useState(false);
  const [removing, setRemoving] = useState<Grant>();
  const links = useQuery({
    queryKey: [...queryKey, page],
    queryFn: () => apiGet<{ available: boolean; grants: Grant[]; total: number; page_size: number }>(api, { page }),
    placeholderData: keepPreviousData,
  });
  const create = useMutation({
    mutationFn: () => apiPost<{ grant: Grant; token: string }>(api, {
      peer_id: peerId.trim(),
      duration_minutes: duration,
      max_uses: maxUses,
    }),
    onSuccess: ({ grant, token }) => {
      const url = new URL(window.location.pathname, window.location.origin);
      url.hash = `/guest-connect?${new URLSearchParams({ token, peer: grant.peer_id })}`;
      setNewLink({ id: grant.id, url: url.toString() });
      setCopied(false);
      setPage(1);
      void qc.invalidateQueries({ queryKey });
    },
  });
  const revoke = useMutation({
    mutationFn: (id: number) => http.delete(`${api}/${id}`),
    onSuccess: (_, id) => {
      if (newLink?.id === id) setNewLink(undefined);
      setRemoving(undefined);
      if (page > 1 && links.data?.grants.length === 1) setPage(page - 1);
      void qc.invalidateQueries({ queryKey });
    },
  });

  if (!links.data?.available) return null;

  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-4 sm:p-5">
      <h2 className="text-base font-semibold">{t("webGuestTitle")}</h2>
      <p className="mt-1 max-w-prose text-sm leading-6 text-kumo-subtle">{t("webGuestHint")}</p>
      <form className="mt-4 flex flex-wrap items-end gap-3" onSubmit={(event) => {
        event.preventDefault();
        if (peerId.trim() && !create.isPending) create.mutate();
      }}>
        <label className="grid min-w-36 gap-1.5 text-sm font-medium">
          {t("deviceId")}
          <Input value={peerId} maxLength={128} onChange={(event) => setPeerId(event.target.value)} />
        </label>
        <label className="grid gap-1.5 text-sm font-medium">
          {t("webGuestDuration")}
          <select className="min-h-10 rounded-md border border-kumo-line bg-kumo-base px-2" value={duration}
            onChange={(event) => setDuration(Number(event.target.value))}>
            {durations.map((minutes) => (
              <option key={minutes} value={minutes}>{minutes < 60
                ? t("webGuestShortDuration", { minutes })
                : t("webGuestLongDuration", { hours: minutes / 60 })}</option>
            ))}
          </select>
        </label>
        <label className="grid gap-1.5 text-sm font-medium">
          {t("webGuestMaxUses")}
          <select className="min-h-10 rounded-md border border-kumo-line bg-kumo-base px-2" value={maxUses}
            onChange={(event) => setMaxUses(Number(event.target.value))}>
            {Array.from({ length: 10 }, (_, index) => index + 1).map((uses) => <option key={uses}>{uses}</option>)}
          </select>
        </label>
        <Button type="submit" variant="primary" disabled={!peerId.trim() || create.isPending} loading={create.isPending}>
          {t("webGuestCreate")}
        </Button>
      </form>
      {create.error && <InlineMessage className="mt-3" tone="error">{create.error.message}</InlineMessage>}
      {newLink && (
        <div className="mt-4 flex flex-wrap items-center gap-3 rounded-lg border border-kumo-line bg-kumo-base p-3">
          <p className="text-sm" role="status">{t("webGuestLinkOnce")}</p>
          <Button variant="secondary" onClick={() => { void copyToClipboard(newLink.url).then(setCopied); }}>
            {t(copied ? "copied" : "webGuestCopy")}
          </Button>
        </div>
      )}
      {links.data.grants.length > 0 ? (
        <ul className="mt-4 divide-y divide-kumo-line border-t border-kumo-line">
          {links.data.grants.map((grant) => (
            <li key={grant.id} className="flex flex-wrap items-center justify-between gap-2 py-3">
              <div>
                <div className="text-sm font-medium">{t("webGuestDevice", { id: grant.peer_id })}</div>
                <div className="mt-1 text-xs text-kumo-subtle">
                  {formatUnixSeconds(grant.expires_at)} · {t("webGuestUsage", { used: grant.used_count, max: grant.max_uses })}
                </div>
              </div>
              <Button size="sm" variant="ghost" onClick={() => { revoke.reset(); setRemoving(grant); }}>{t("webGuestRevoke")}</Button>
            </li>
          ))}
        </ul>
      ) : <p className="mt-4 border-t border-kumo-line pt-3 text-sm text-kumo-subtle">{t("webGuestEmpty")}</p>}
      {(page > 1 || links.data.total > links.data.page_size) && (
        <div className="mt-2 flex justify-end gap-2">
          <Button size="sm" variant="secondary" onClick={() => setPage(page - 1)} disabled={page <= 1 || links.isFetching}>
            {t("consolePreviousPage")}
          </Button>
          <Button size="sm" variant="secondary" onClick={() => setPage(page + 1)} disabled={page * links.data.page_size >= links.data.total || links.isFetching}>
            {t("consoleNextPage")}
          </Button>
        </div>
      )}
      {removing && (
        <ConfirmDialog open destructive title={t("webGuestRevoke")} description={t("webGuestRevokeHint")}
          confirmLabel={t("webGuestRevoke")} cancelLabel={t("cancel")}
          loading={revoke.isPending} error={revoke.error?.message}
          onConfirm={() => revoke.mutate(removing.id)}
          onOpenChange={(open) => { if (!open) setRemoving(undefined); }} />
      )}
    </section>
  );
}
