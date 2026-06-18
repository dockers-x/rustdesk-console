import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { apiPost, ApiError } from "../lib/api";

function openNativeClient(peerId: string) {
  const a = document.createElement("a");
  a.href = `rustdesk://${peerId}`;
  a.rel = "noreferrer";
  document.body.appendChild(a);
  a.click();
  a.remove();
}

export function PeerQuickActions({
  peerId,
  rowId,
}: {
  peerId: string;
  rowId?: number;
}) {
  const { t } = useTranslation();
  const [message, setMessage] = useState("");
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

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
      await apiPost("/api/admin/peer/sysinfo-refresh", { row_id: rowId });
      setMessage(t("refreshDeviceInfoSent"));
    } catch (err) {
      const ae = err as ApiError;
      setMessage(ae.message || t("operationFailed"));
    } finally {
      setRefreshing(false);
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
        onClick={() =>
          window.open(
            `${window.location.origin}/webclient/#/${encodeURIComponent(peerId)}`,
            "_blank",
            "noopener,noreferrer",
          )
        }
      >
        {t("webClient")}
      </Button>
      <Button
        size="sm"
        variant="ghost"
        disabled={loading}
        onClick={addToAddressBook}
      >
        {t("addToAddressBook")}
      </Button>
      <Button
        size="sm"
        variant="ghost"
        disabled={refreshing}
        onClick={requestSysinfoRefresh}
      >
        {t("refreshDeviceInfo")}
      </Button>
      {message && <span className="text-xs text-kumo-subtle">{message}</span>}
    </div>
  );
}
