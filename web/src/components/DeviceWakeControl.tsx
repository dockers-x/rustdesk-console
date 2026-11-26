import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { ApiError, apiGet, apiPost, apiPut } from "../lib/api";
import { getAuthSessionId } from "../lib/auth";

interface WakeSettings {
  mac_address: string | null;
  manufacturer?: string;
  network_configured: boolean;
  device_enabled: boolean;
}

const HEARTBEAT_WAIT_MS = 90_000;

export function DeviceWakeControl({
  rowId,
  lastHeartbeat,
}: {
  rowId: number;
  lastHeartbeat: number;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const inputId = useId();
  const [draft, setDraft] = useState<string | null>(null);
  const [watch, setWatch] = useState<{ sentAt: number; expired: boolean } | null>(null);
  const endpoint = `/api/admin/peer/wake-on-lan/${rowId}`;
  const queryKey = ["device-wake", getAuthSessionId(), rowId];
  const settings = useQuery({
    queryKey,
    queryFn: ({ signal }) => apiGet<WakeSettings>(endpoint, undefined, signal),
  });
  const save = useMutation({
    mutationFn: (value: string) => apiPut<WakeSettings>(endpoint, { mac_address: value }),
    onSuccess: (updated) => {
      client.setQueryData(queryKey, updated);
      setDraft(null);
      setWatch(null);
      send.reset();
    },
  });
  const send = useMutation({
    mutationFn: () => apiPost<{ sent_at: number }>(`${endpoint}/send`, {}),
    retry: false,
    onSuccess: ({ sent_at }) => setWatch({ sentAt: sent_at, expired: false }),
  });
  const heartbeatObserved = watch !== null && lastHeartbeat > watch.sentAt;

  useEffect(() => {
    if (watch === null || watch.expired || heartbeatObserved) return;
    const timer = window.setTimeout(
      () => setWatch({ ...watch, expired: true }),
      HEARTBEAT_WAIT_MS,
    );
    return () => window.clearTimeout(timer);
  }, [watch, heartbeatObserved]);

  const registered = settings.data?.mac_address ?? "";
  const value = draft ?? registered;
  const changed = value.trim() !== registered;
  const busy = save.isPending || send.isPending;
  const actionError = save.error ?? send.error;
  const feedback = watch
    ? t(heartbeatObserved ? "wakeHeartbeatObserved" : watch.expired ? "wakeHeartbeatPending" : "wakeHeartbeatWaiting")
    : save.isSuccess ? t("wakeMacSaved") : null;
  const unavailable = settings.data && (
    !settings.data.network_configured ? t("wakeNetworkMissing") :
    !settings.data.device_enabled ? t("wakeDeviceDisabled") :
    null
  );

  return (
    <section className="mt-5 grid gap-3" aria-labelledby={`${inputId}-heading`}>
      <h3 id={`${inputId}-heading`} className="text-sm font-semibold">{t("wakeOnLan")}</h3>
      {settings.data ? (
        <div className="grid gap-3 rounded-md border border-kumo-line bg-kumo-base px-3 py-3">
          <form
            className="grid items-end gap-2 sm:grid-cols-[minmax(0,1fr)_auto]"
            onSubmit={(event) => {
              event.preventDefault();
              if (!busy && changed) save.mutate(value);
            }}
          >
            <div className="grid min-w-0 gap-1.5">
              <label htmlFor={inputId} className="text-xs font-medium text-kumo-subtle">
                {t("wakeMacAddress")}
              </label>
              <Input
                id={inputId}
                type="text"
                className="min-h-11 font-mono"
                placeholder="00:1B:63:12:34:56"
                autoComplete="off"
                autoCapitalize="characters"
                spellCheck={false}
                aria-describedby={`${inputId}-hint`}
                aria-invalid={save.isError || undefined}
                maxLength={30}
                value={value}
                disabled={busy}
                onChange={(event) => {
                  save.reset();
                  send.reset();
                  setDraft(event.target.value);
                }}
              />
            </div>
            <Button type="submit" variant="secondary" className="min-h-11" disabled={busy || !changed}>
              {t("save")}
            </Button>
          </form>
          <p id={`${inputId}-hint`} className="text-xs leading-5 text-kumo-subtle">
            {t("wakeMacHint")}
          </p>
          {settings.data.manufacturer && !changed && (
            <p className="text-sm">{t("wakeManufacturer", { name: settings.data.manufacturer })}</p>
          )}
          <div className="grid gap-2 border-t border-kumo-line pt-3">
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                variant="secondary"
                className="min-h-11"
                disabled={busy || changed || !registered || Boolean(unavailable)}
                onClick={() => {
                  save.reset();
                  setWatch(null);
                  send.mutate();
                }}
              >
                {t(send.isPending ? "wakeSending" : "wakeSend")}
              </Button>
              {unavailable && <p className="text-xs leading-5 text-kumo-subtle">{unavailable}</p>}
            </div>
            {actionError ? (
              <p role="alert" className="text-sm text-kumo-danger">
                {actionError instanceof ApiError ? actionError.message : t("operationFailed")}
              </p>
            ) : feedback && (
              <p role="status" className="text-sm text-kumo-subtle">{feedback}</p>
            )}
          </div>
        </div>
      ) : settings.isError ? (
        <div className="flex items-center gap-3">
          <p role="alert" className="text-sm text-kumo-danger">{t("operationFailed")}</p>
          <Button type="button" variant="secondary" onClick={() => void settings.refetch()}>
            {t("retryLoad")}
          </Button>
        </div>
      ) : (
        <p role="status" className="text-sm text-kumo-subtle">{t("loading")}</p>
      )}
    </section>
  );
}
