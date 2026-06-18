import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "./DialogLayout";
import { apiPost, ApiError } from "../lib/api";
import {
  getPreferredWebClientVersion,
  openWebClientPeer,
  webClientShareUrl,
  type WebClientVersion,
} from "../lib/rustdeskLinks";

const EXPIRE_OPTIONS = [
  { value: 300, labelKey: "expire5m" },
  { value: 1800, labelKey: "expire30m" },
  { value: 3600, labelKey: "expire1h" },
  { value: 86400, labelKey: "expire1d" },
  { value: 604800, labelKey: "expire1w" },
  { value: 2592000, labelKey: "expire1mo" },
  { value: 0, labelKey: "expireForever" },
];

interface ShareResult {
  share_token: string;
}

async function copyText(value: string) {
  if (!value) return;
  await navigator.clipboard?.writeText(value);
}

export function WebClientActions({
  peerId,
  share = false,
  version,
}: {
  peerId: string;
  share?: boolean;
  version?: WebClientVersion;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [expire, setExpire] = useState(1800);
  const [link, setLink] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [opening, setOpening] = useState(false);

  const reset = () => {
    setPassword("");
    setExpire(1800);
    setLink("");
    setError("");
    setLoading(false);
    setOpening(false);
  };

  const resolveVersion = async () => version ?? (await getPreferredWebClientVersion());

  const openWebClient = async () => {
    setOpening(true);
    try {
      await openWebClientPeer(peerId, version);
    } finally {
      setOpening(false);
    }
  };

  const submitShare = async () => {
    if (!password.trim()) {
      setError(t("passwordRequired"));
      return;
    }
    setError("");
    setLoading(true);
    try {
      const res = await apiPost<ShareResult>(
        "/api/admin/address_book/shareByWebClient",
        {
          id: peerId,
          password_type: "once",
          password,
          expire,
        },
      );
      const selectedVersion = await resolveVersion();
      const url = webClientShareUrl(res.share_token, selectedVersion);
      setLink(url);
      await copyText(url).catch(() => undefined);
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  if (!peerId) return null;

  return (
    <div className="flex flex-wrap gap-1">
      <Button
        size="sm"
        variant="ghost"
        disabled={opening}
        onClick={() => void openWebClient()}
      >
        {t("webClient")}
      </Button>
      {share && (
        <Dialog.Root
          open={open}
          onOpenChange={(next) => {
            setOpen(next);
            if (!next) reset();
          }}
        >
          <Button size="sm" variant="ghost" onClick={() => setOpen(true)}>
            {t("share")}
          </Button>
          <Dialog size="lg" className={dialogPanelClass}>
            <DialogHeader
              title={t("shareByWebClient")}
              description={t("shareWebClientHint")}
            />
            <DialogBody>
              <div className="grid gap-4">
                <div className="rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm">
                  <span className="text-kumo-subtle">{t("deviceId")}: </span>
                  <span className="break-all font-medium">{peerId}</span>
                </div>
                <label className="block">
                  <span className="mb-1 block text-sm">{t("password")}</span>
                  <Input
                    aria-label={t("password")}
                    type="password"
                    value={password}
                    disabled={Boolean(link)}
                    onChange={(e) => setPassword(e.target.value)}
                  />
                </label>
                <label className="block">
                  <span className="mb-1 block text-sm">{t("expire")}</span>
                  <select
                    className="h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                    value={expire}
                    disabled={Boolean(link)}
                    onChange={(e) => setExpire(Number(e.target.value))}
                  >
                    {EXPIRE_OPTIONS.map((o) => (
                      <option key={o.value} value={o.value}>
                        {t(o.labelKey)}
                      </option>
                    ))}
                  </select>
                </label>
                {link && (
                  <label className="block">
                    <span className="mb-1 block text-sm">{t("link")}</span>
                    <div className="flex flex-col gap-2 sm:flex-row">
                      <Input aria-label={t("link")} value={link} readOnly />
                      <Button
                        variant="secondary"
                        onClick={() => void copyText(link)}
                      >
                        {t("copy")}
                      </Button>
                    </div>
                  </label>
                )}
              </div>
            </DialogBody>
            <DialogFooter error={error || undefined}>
              <Button variant="secondary" onClick={() => setOpen(false)}>
                {link ? t("close") : t("cancel")}
              </Button>
              {!link && (
                <Button onClick={submitShare} loading={loading}>
                  {t("createShare")}
                </Button>
              )}
            </DialogFooter>
          </Dialog>
        </Dialog.Root>
      )}
    </div>
  );
}
