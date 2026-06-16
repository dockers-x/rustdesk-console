import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { ConfirmDialog } from "./ConfirmDialog";
import { apiPost, ApiError } from "../lib/api";

export function ActiveConnectionActions({
  connId,
  peerId,
  uuid,
}: {
  connId: number;
  peerId: string;
  uuid: string;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const disconnect = async () => {
    setLoading(true);
    setError("");
    setMessage("");
    try {
      await apiPost("/api/admin/peer/disconnect", {
        peer_id: peerId,
        uuid,
        conn_ids: [connId],
      });
      setOpen(false);
      setMessage(t("disconnectSent"));
      void qc.invalidateQueries({ queryKey: ["active_connection"] });
    } catch (err) {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="inline-flex min-w-32 flex-col items-start gap-1">
      <Button
        size="sm"
        variant="secondary-destructive"
        className="min-w-16 whitespace-nowrap"
        disabled={!connId || loading}
        aria-label={t("disconnectConnection", { connId })}
        title={t("disconnectConnection", { connId })}
        onClick={() => setOpen(true)}
      >
        {t("disconnect")}
      </Button>
      {message && (
        <span
          aria-live="polite"
          role="status"
          className="whitespace-nowrap text-xs text-kumo-subtle"
        >
          {message}
        </span>
      )}
      <ConfirmDialog
        open={open}
        title={t("confirmDisconnectTitle")}
        description={t("confirmDisconnectDescription", { connId })}
        confirmLabel={t("disconnect")}
        cancelLabel={t("cancel")}
        error={error}
        loading={loading}
        onOpenChange={setOpen}
        onConfirm={disconnect}
      />
    </div>
  );
}
