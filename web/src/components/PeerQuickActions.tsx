import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { ShieldCheck } from "@phosphor-icons/react";
import { apiPost, ApiError } from "../lib/api";
import {
  openWebClientPeer,
  rustdeskNativeUri,
} from "../lib/rustdeskLinks";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "./DialogLayout";
import { InlineMessage } from "./InlineMessage";

type PeerActionScope = "admin" | "my";

interface TrustedDevicesResult {
  peer_id: string;
  strategy_id: number;
  strategy_name: string;
  already_enabled: boolean;
}

function openNativeClient(peerId: string) {
  const a = document.createElement("a");
  a.href = rustdeskNativeUri(peerId);
  a.rel = "noreferrer";
  document.body.appendChild(a);
  a.click();
  a.remove();
}

export function PeerQuickActions({
  peerId,
  rowId,
  scope = "my",
  showAddressBook = true,
}: {
  peerId: string;
  rowId?: number;
  scope?: PeerActionScope;
  showAddressBook?: boolean;
}) {
  const { t } = useTranslation();
  const [message, setMessage] = useState("");
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [openingWebClient, setOpeningWebClient] = useState(false);
  const [trustedOpen, setTrustedOpen] = useState(false);
  const [trustedLoading, setTrustedLoading] = useState(false);
  const [trustedResult, setTrustedResult] =
    useState<TrustedDevicesResult | null>(null);
  const [trustedError, setTrustedError] = useState("");

  const peerBase = scope === "admin" ? "/api/admin/peer" : "/api/admin/my/peer";

  const resetTrustedDialog = () => {
    setTrustedResult(null);
    setTrustedError("");
    setTrustedLoading(false);
  };

  const addToAddressBook = async () => {
    if (!rowId) {
      setMessage(t("missingPeerRow"));
      return;
    }
    setLoading(true);
    setMessage("");
    try {
      await apiPost("/api/admin/my/address_book/batchCreateFromPeers", {
        collection_id: 0,
        peer_ids: [rowId],
        tags: [],
      });
      setMessage(t("addedToAddressBook"));
    } catch (err) {
      const ae = err as ApiError;
      setMessage(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  const requestSysinfoRefresh = async () => {
    if (!rowId) {
      setMessage(t("missingPeerRow"));
      return;
    }
    setRefreshing(true);
    setMessage("");
    try {
      await apiPost(`${peerBase}/sysinfo-refresh`, { row_id: rowId });
      setMessage(t("refreshDeviceInfoSent"));
    } catch (err) {
      const ae = err as ApiError;
      setMessage(ae.message || t("operationFailed"));
    } finally {
      setRefreshing(false);
    }
  };

  const enableTrustedDevices = async () => {
    if (!rowId) {
      setTrustedError(t("missingPeerRow"));
      return;
    }
    setTrustedLoading(true);
    setTrustedError("");
    try {
      const res = await apiPost<TrustedDevicesResult>(
        `${peerBase}/trusted-devices/enable`,
        { row_id: rowId },
      );
      setTrustedResult(res);
      setMessage(
        res.already_enabled
          ? t("trustedDevicePolicyAlreadyEnabled")
          : t("trustedDevicePolicyEnabled"),
      );
    } catch (err) {
      const ae = err as ApiError;
      setTrustedError(ae.message || t("operationFailed"));
    } finally {
      setTrustedLoading(false);
    }
  };

  const openWebClient = async () => {
    setOpeningWebClient(true);
    try {
      await openWebClientPeer(peerId);
    } finally {
      setOpeningWebClient(false);
    }
  };

  if (!peerId) return null;

  return (
    <div className="flex flex-wrap items-center gap-1">
      <Button size="sm" variant="ghost" onClick={() => openNativeClient(peerId)}>
        {t("client")}
      </Button>
      <Button
        size="sm"
        variant="ghost"
        disabled={openingWebClient}
        onClick={() => void openWebClient()}
      >
        {t("webClient")}
      </Button>
      {showAddressBook && (
        <Button
          size="sm"
          variant="ghost"
          disabled={loading}
          onClick={addToAddressBook}
        >
          {t("addToAddressBook")}
        </Button>
      )}
      <Button
        size="sm"
        variant="ghost"
        disabled={refreshing}
        onClick={requestSysinfoRefresh}
      >
        {t("refreshDeviceInfo")}
      </Button>
      <Dialog.Root
        open={trustedOpen}
        onOpenChange={(next) => {
          setTrustedOpen(next);
          if (!next) resetTrustedDialog();
        }}
      >
        <Button
          size="sm"
          variant="ghost"
          className="gap-1.5"
          onClick={() => setTrustedOpen(true)}
        >
          <ShieldCheck size={14} aria-hidden />
          {t("trustedDevicePolicy")}
        </Button>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader
            title={t("enableTrustedDevicesForPeer")}
            description={t("trustedDevicePolicyDialogDescription")}
          />
          <DialogBody>
            <div className="grid gap-4">
              <div className="rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-sm">
                <span className="text-kumo-subtle">{t("deviceId")}: </span>
                <span className="break-all font-medium">{peerId}</span>
              </div>
              <div className="rounded-md border border-kumo-warning/25 bg-kumo-warning-tint/40 px-3 py-3 text-sm leading-6 text-kumo-default">
                <p className="font-medium">{t("trustedDevicePolicyBoundary")}</p>
                <p className="mt-1 text-kumo-subtle">
                  {t("trustedDevicePolicyBoundaryHint")}
                </p>
              </div>
              <ol className="grid gap-2 text-sm leading-6 text-kumo-subtle">
                <li className="flex gap-2">
                  <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border border-kumo-line bg-kumo-base text-xs text-kumo-default">
                    1
                  </span>
                  <span>{t("trustedDevicePolicyStep1")}</span>
                </li>
                <li className="flex gap-2">
                  <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border border-kumo-line bg-kumo-base text-xs text-kumo-default">
                    2
                  </span>
                  <span>{t("trustedDevicePolicyStep2")}</span>
                </li>
                <li className="flex gap-2">
                  <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border border-kumo-line bg-kumo-base text-xs text-kumo-default">
                    3
                  </span>
                  <span>{t("trustedDevicePolicyStep3")}</span>
                </li>
              </ol>
              {trustedResult && (
                <InlineMessage tone="success">
                  <div className="grid gap-1">
                    <span>
                      {trustedResult.already_enabled
                        ? t("trustedDevicePolicyAlreadyEnabled")
                        : t("trustedDevicePolicyEnabled")}
                    </span>
                    <span className="break-all text-xs">
                      {t("strategyId")}: {trustedResult.strategy_id} ·{" "}
                      {trustedResult.strategy_name}
                    </span>
                    <span className="text-xs">
                      {t("trustedDevicePolicyNextStep")}
                    </span>
                  </div>
                </InlineMessage>
              )}
            </div>
          </DialogBody>
          <DialogFooter error={trustedError || undefined}>
            <Button variant="secondary" onClick={() => setTrustedOpen(false)}>
              {trustedResult ? t("close") : t("cancel")}
            </Button>
            {!trustedResult && (
              <Button
                loading={trustedLoading}
                onClick={() => void enableTrustedDevices()}
              >
                {t("enableTrustedDevicesForPeer")}
              </Button>
            )}
          </DialogFooter>
        </Dialog>
      </Dialog.Root>
      {message && <span className="text-xs text-kumo-subtle">{message}</span>}
    </div>
  );
}
