import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Info } from "@phosphor-icons/react";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Banner } from "@cloudflare/kumo/components/banner";
import { Button, LinkButton } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { LayerDialog } from "@cloudflare/kumo/components/layer-dialog";
import { Switch } from "@cloudflare/kumo/components/switch";
import { Table } from "@cloudflare/kumo/components/table";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { TableState } from "../components/TableState";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "../components/DialogLayout";
import { apiGet, apiPost, apiPut } from "../lib/api";
import { formatDateTime } from "../lib/dateFormat";

interface AdmissionSettings {
  require_deployment: boolean;
  paired_token_configured: boolean;
  hbbs_connected: boolean;
}

interface DeviceReview {
  device_id: string;
  fingerprint: string;
  status: "pending" | "approved" | "revoked";
  updated_at: string;
}

const admissionKey = ["device-admission"];
const base = "/api/admin/device-admission";
const pageSize = 20;
const deploymentCommand = 'rustdesk --deploy --token "DEPLOYMENT_TOKEN"';

function AdmissionHelp() {
  const { t } = useTranslation();
  return (
    <LayerDialog>
      <LayerDialog.Trigger render={<Button shape="square" className="max-sm:size-11" variant="ghost" aria-label={t("admissionHelp")} />}>
        <Info size={20} aria-hidden />
      </LayerDialog.Trigger>
      <LayerDialog.Content size="lg" closeLabel={t("close")}>
        <LayerDialog.Title>{t("admissionHelp")}</LayerDialog.Title>
        <LayerDialog.Description>{t("requireDeviceDeploymentHint")}</LayerDialog.Description>
        <LayerDialog.Body>
          <div className="grid gap-5 text-sm leading-6 [overflow-wrap:anywhere]">
            <section>
              <h3 className="mb-2 font-semibold">{t("admissionHelpServer")}</h3>
              <ol className="list-decimal space-y-2 pl-5 marker:text-kumo-subtle">
                <li>{t("admissionHelpPair", { token: "RUSTDESK_API_SECURITY_HBBS_ADMISSION_TOKEN" })}</li>
                <li>{t("admissionHelpApi", { address: "RUSTDESK_API_RUSTDESK_API_SERVER" })}</li>
                <li>{t("admissionHelpEnable")}</li>
              </ol>
            </section>
            <section className="border-t border-kumo-line pt-4">
              <h3 className="mb-2 font-semibold">{t("admissionHelpClient")}</h3>
              <ol className="list-decimal space-y-2 pl-5 marker:text-kumo-subtle">
                <li>{t("admissionHelpNetwork")}</li>
                <li>{t("admissionHelpManual")}</li>
                <li>
                  {t("admissionHelpToken")}
                  <code className="my-2 block whitespace-normal break-words rounded-md bg-kumo-recessed px-3 py-2">{deploymentCommand}</code>
                  {t("admissionHelpAndroid")}
                </li>
              </ol>
            </section>
            <p className="rounded-md bg-kumo-recessed px-3 py-2 text-kumo-subtle">{t("admissionHelpLimits")}</p>
          </div>
        </LayerDialog.Body>
      </LayerDialog.Content>
    </LayerDialog>
  );
}

export function DeviceAdmissionPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toasts = useKumoToastManager();
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);
  const [enable, setEnable] = useState<boolean | null>(null);
  const [review, setReview] = useState<{ device: DeviceReview; approved: boolean }>();
  const settings = useQuery({
    queryKey: [...admissionKey, "settings"],
    queryFn: () => apiGet<AdmissionSettings>(`${base}/settings`),
    refetchInterval: 5_000,
  });
  const devices = useQuery({
    queryKey: [...admissionKey, "devices", search, page],
    queryFn: () => apiGet<{ list: DeviceReview[]; total: number }>(`${base}/list`, { device_id: search, page, page_size: pageSize }),
    refetchInterval: 15_000,
  });
  const saveSetting = useMutation({
    mutationFn: (value: boolean) => apiPut(`${base}/settings`, { require_deployment: value }),
    onSuccess: () => {
      setEnable(null);
      void qc.invalidateQueries({ queryKey: admissionKey });
      toasts.add({ title: t("operationSuccess"), variant: "success" });
    },
  });
  const submitReview = useMutation({
    mutationFn: ({ device, approved }: NonNullable<typeof review>) => apiPost(`${base}/review`, {
      device_id: device.device_id,
      fingerprint: device.fingerprint,
      approved,
    }),
    onSuccess: () => {
      setReview(undefined);
      void qc.invalidateQueries({ queryKey: admissionKey });
      toasts.add({ title: t("operationSuccess"), variant: "success" });
    },
  });

  return (
    <div className="mx-auto grid max-w-[96rem] gap-5">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <h1 className="text-2xl font-semibold">{t("deviceAdmission")}</h1>
          <AdmissionHelp />
        </div>
        <LinkButton variant="secondary" href="#/deployment_tokens">
          {t("deploymentTokens")}
        </LinkButton>
      </header>
      <section className="grid gap-3 rounded-lg border border-kumo-line bg-kumo-elevated p-4">
        <Switch
          label={t("requireDeviceDeployment")}
          aria-describedby="device-deployment-hint"
          checked={settings.data?.require_deployment ?? false}
          disabled={(!settings.data?.hbbs_connected && !settings.data?.require_deployment) || saveSetting.isPending}
          onCheckedChange={(value) => { saveSetting.reset(); setEnable(value); }}
        />
        <p id="device-deployment-hint" className="text-sm text-kumo-subtle">{t("requireDeviceDeploymentHint")}</p>
        {settings.data && !settings.data.paired_token_configured && (
          <Banner variant="alert" title={t("admissionPairingRequired")} description={<span className="[overflow-wrap:anywhere]">{t("admissionPairingHint")}</span>} />
        )}
        {settings.data?.paired_token_configured && !settings.data.hbbs_connected && (
          <Banner variant="alert" title={t("admissionPairingWaiting")} description={<span className="[overflow-wrap:anywhere]">{t("admissionPairingHint")}</span>} />
        )}
        {settings.error && <TableState tone="error">{settings.error.message}</TableState>}
      </section>
      <section className="min-w-0 rounded-lg border border-kumo-line bg-kumo-elevated">
        <div className="flex flex-wrap items-end justify-between gap-3 p-4">
          <Input label={t("deviceId")} value={search} onChange={(event) => { setSearch(event.target.value); setPage(1); }} />
          <Button variant="secondary" onClick={() => void devices.refetch()}>{t("refresh")}</Button>
        </div>
        <div className="overflow-x-auto">
          <Table className="min-w-max">
            <Table.Header><Table.Row>
              {["deviceId", "admissionFingerprint", "status", "updatedAt", "actions"].map((key) => <Table.Head key={key}>{t(key)}</Table.Head>)}
            </Table.Row></Table.Header>
            <Table.Body>
              {devices.data?.list.map((device) => (
                <Table.Row key={device.device_id}>
                  <Table.Cell>{device.device_id}</Table.Cell>
                  <Table.Cell><code className="text-xs">{device.fingerprint}</code></Table.Cell>
                  <Table.Cell><Badge>{t({
                    approved: "admissionApproved", pending: "admissionPending", revoked: "admissionRevoked",
                  }[device.status])}</Badge></Table.Cell>
                  <Table.Cell>{formatDateTime(device.updated_at)}</Table.Cell>
                  <Table.Cell>
                    <div className="flex gap-1">
                      {device.status !== "approved" && <Button size="sm" variant="secondary"
                        onClick={() => { submitReview.reset(); setReview({ device, approved: true }); }}>
                        {t("admissionApprove")}
                      </Button>}
                      {device.status !== "revoked" && <Button size="sm" variant="ghost"
                        onClick={() => { submitReview.reset(); setReview({ device, approved: false }); }}>
                        {t("bookRevoke")}
                      </Button>}
                    </div>
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
        </div>
        {devices.isPending && <TableState tone="loading">{t("loading")}</TableState>}
        {devices.error && <TableState tone="error">{devices.error.message}</TableState>}
        {devices.data?.total === 0 && <TableState tone="empty">{t("noData")}</TableState>}
        {devices.data && <div className="flex justify-end gap-2 p-4">
          <Button size="sm" variant="secondary" disabled={page === 1} onClick={() => setPage(page - 1)}>{t("bookPrevious")}</Button>
          <Button size="sm" variant="secondary" disabled={page * pageSize >= devices.data.total} onClick={() => setPage(page + 1)}>{t("bookNext")}</Button>
        </div>}
      </section>
      {enable !== null && <ConfirmDialog open title={t("requireDeviceDeployment")}
        description={t(enable ? "admissionEnableDescription" : "admissionDisableDescription")}
        confirmLabel={t("save")} cancelLabel={t("cancel")} loading={saveSetting.isPending}
        error={saveSetting.error?.message} onConfirm={() => saveSetting.mutate(enable)}
        onOpenChange={(open) => { if (!open) setEnable(null); }} />}
      {review && <ConfirmDialog open title={t(review.approved ? "admissionApprove" : "bookRevoke")}
        description={t(review.approved ? "admissionApproveDescription" : "admissionRevokeDescription", { id: review.device.device_id })}
        confirmLabel={t(review.approved ? "admissionApprove" : "bookRevoke")} cancelLabel={t("cancel")}
        error={submitReview.error?.message} loading={submitReview.isPending} onConfirm={() => submitReview.mutate(review)}
        onOpenChange={(open) => { if (!open) setReview(undefined); }} />}
    </div>
  );
}

interface ControlRole {
  deny_keyboard: boolean;
  deny_clipboard: boolean;
  deny_file: boolean;
}

const controls = [
  ["deny_keyboard", "controlDenyKeyboard"],
  ["deny_clipboard", "controlDenyClipboard"],
  ["deny_file", "controlDenyFile"],
] as const;

export function UserControlRoleAction({ userId, username }: { userId: number; username: string }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<ControlRole>();
  const roleKey = ["control-role", userId];
  const url = `/api/admin/user/control-role/${userId}`;
  const role = useQuery({
    queryKey: roleKey,
    queryFn: () => apiGet<ControlRole>(url),
    enabled: open,
  });
  const save = useMutation({
    mutationFn: (value: ControlRole) => apiPut(url, value),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: roleKey });
      setOpen(false);
    },
  });
  const value = draft ?? role.data;

  return (
    <Dialog.Root open={open} onOpenChange={(next) => {
      if (save.isPending) return;
      setOpen(next);
      if (next) { setDraft(undefined); save.reset(); }
    }}>
      <Button size="sm" variant="ghost" onClick={() => { setDraft(undefined); save.reset(); setOpen(true); }}>{t("controlRole")}</Button>
      <Dialog size="sm" className={dialogPanelClass}>
        <DialogHeader title={t("controlRole")} description={username} />
        <DialogBody>
          <p className="mb-4 text-sm text-kumo-subtle">{t("controlRoleHint")}</p>
          {role.isPending && <TableState tone="loading">{t("loading")}</TableState>}
          {role.error && <TableState tone="error">{role.error.message}</TableState>}
          {value && <div className="grid gap-3">
            {controls.map(([key, label]) => (
              <Switch key={key} label={t(label)} checked={value[key]}
                onCheckedChange={(checked) => setDraft({ ...value, [key]: checked })} />
            ))}
          </div>}
        </DialogBody>
        <DialogFooter error={save.error?.message}>
          <Button variant="secondary" onClick={() => setOpen(false)} disabled={save.isPending}>{t("cancel")}</Button>
          <Button onClick={() => { if (draft) save.mutate(draft); }} disabled={!draft || save.isPending} loading={save.isPending}>{t("save")}</Button>
        </DialogFooter>
      </Dialog>
    </Dialog.Root>
  );
}
