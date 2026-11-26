import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Banner } from "@cloudflare/kumo/components/banner";
import { Button } from "@cloudflare/kumo/components/button";
import { Combobox } from "@cloudflare/kumo/components/combobox";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Select } from "@cloudflare/kumo/components/select";
import { useKumoToastManager } from "@cloudflare/kumo/components/toast";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "../components/DialogLayout";
import { TableState } from "../components/TableState";
import { apiGet, apiPost, apiPut, http } from "../lib/api";
import { getAuthSessionId } from "../lib/auth";

const base = "/api/admin/blocklist";
const kinds = ["device", "ip", "cidr"] as const;
type Kind = (typeof kinds)[number];
type SyncState = "waiting" | "updating" | "synced";
type RuleFields = { kind: Kind; value: string; note: string };
type Rule = RuleFields & { id: number };
type Draft = RuleFields & { id?: number };

interface Blocklist {
  rules: Rule[];
  paired_token_configured: boolean;
  hbbs: SyncState;
  hbbr: SyncState;
}

interface DeviceChoice {
  id: string;
  hostname: string;
  alias: string;
  last_online_ip: string;
}

const emptyDraft: Draft = { kind: "device", value: "", note: "" };
const pageSize = 25;
const kindLabels = { device: "blocklistDevice", ip: "blocklistIp", cidr: "blocklistCidr" } as const;
const valueLabels = { device: "deviceId", ip: "blocklistIp", cidr: "blocklistCidr" } as const;
const kindHints = { device: "blocklistDeviceHint", ip: "blocklistIpHint", cidr: "blocklistCidrHint" } as const;
const invalidLabels = { device: "blocklistInvalidDevice", ip: "blocklistInvalidIp", cidr: "blocklistInvalidCidr" } as const;
const syncLabels = { waiting: "blocklistWaiting", updating: "blocklistUpdating", synced: "blocklistSynced" } as const;

function ipPrefixLimit(value: string) {
  if (/^(?:0|[1-9]\d{0,2})(?:\.(?:0|[1-9]\d{0,2})){3}$/.test(value) &&
      value.split(".").every((octet) => Number(octet) <= 255)) return 32;
  if (!value.includes(":")) return 0;
  try {
    return new URL(`http://[${value}]`).hostname.startsWith("[") && /^[\da-f:.]+$/i.test(value) ? 128 : 0;
  } catch {
    return 0;
  }
}

function isValidRule({ kind, value }: RuleFields) {
  const target = value.trim();
  if (kind === "device") {
    return Boolean(target) && new TextEncoder().encode(target).length <= 128 && !/[\p{Cc}\s]/u.test(target);
  }
  if (kind === "ip") return ipPrefixLimit(target) > 0;
  const [address, prefix, extra] = target.split("/");
  const maxPrefix = ipPrefixLimit(address);
  return maxPrefix > 0 && extra === undefined && /^\d{1,3}$/.test(prefix ?? "") && Number(prefix) <= maxPrefix;
}

function DevicePicker({ kind, onSelect }: { kind: Kind; onSelect: (value: string) => void }) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState<DeviceChoice | null>(null);
  const devices = useQuery({
    queryKey: ["blocklist-devices", getAuthSessionId(), search],
    queryFn: ({ signal }) => apiGet<DeviceChoice[]>(`${base}/devices`, { q: search.trim() }, signal),
    enabled: open,
  });
  const items = (devices.data ?? []).filter((device) => kind === "device" || ipPrefixLimit(device.last_online_ip) > 0);
  const deviceName = (device: DeviceChoice) => device.alias || device.hostname || device.id;

  return (
    <Combobox<DeviceChoice>
      label={t(kind === "device" ? "blocklistPickDevice" : "blocklistPickIp")}
      items={items}
      filter={null}
      value={selected}
      open={open}
      onOpenChange={setOpen}
      onInputValueChange={setSearch}
      itemToStringLabel={deviceName}
      itemToStringValue={(device) => device.id}
      isItemEqualToValue={(left, right) => left.id === right.id}
      onValueChange={(device) => {
        setSelected(device);
        if (device) onSelect(kind === "device" ? device.id : `${device.last_online_ip}${kind === "cidr" ? "/" : ""}`);
      }}
    >
      <Combobox.TriggerInput
        placeholder={t("blocklistDeviceSearch")}
        maxLength={128}
        clearLabel={t("blocklistClearDevice")}
        showOptionsLabel={t("blocklistShowDevices")}
      />
      <Combobox.Content>
        <Combobox.List>
          {(device: DeviceChoice) => (
            <Combobox.Item key={device.id} value={device}>
              <span className="grid min-w-0 gap-0.5 break-all text-sm">
                <span>{deviceName(device)}</span>
                <span className="font-mono text-kumo-subtle">
                  {deviceName(device) !== device.id && device.id}
                  {kind !== "device" && ` ${device.last_online_ip}`}
                </span>
              </span>
            </Combobox.Item>
          )}
        </Combobox.List>
        <Combobox.Empty className="px-3 py-2 text-sm text-kumo-subtle">
          {t(devices.isPending ? "loading" : devices.isError ? "operationFailed" : "blocklistNoDevices")}
        </Combobox.Empty>
      </Combobox.Content>
    </Combobox>
  );
}

export function BlocklistPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useKumoToastManager();
  const [draft, setDraft] = useState<Draft>();
  const [invalid, setInvalid] = useState(false);
  const [removing, setRemoving] = useState<Rule>();
  const [page, setPage] = useState(0);
  const key = ["server-blocklist", getAuthSessionId()];
  const blocklist = useQuery({
    queryKey: key,
    queryFn: () => apiGet<Blocklist>(base),
    refetchInterval: 5_000,
  });
  const saved = () => {
    void qc.invalidateQueries({ queryKey: key });
    toast.add({ title: t("operationSuccess"), variant: "success" });
  };
  const save = useMutation({
    mutationFn: ({ id, ...data }: Draft) => id ? apiPut(`${base}/${id}`, data) : apiPost(base, data),
    onSuccess: () => { setDraft(undefined); saved(); },
  });
  const remove = useMutation({
    mutationFn: (id: number) => http.delete(`${base}/${id}`),
    onSuccess: () => { setRemoving(undefined); saved(); },
  });
  const edit = (rule: Draft) => {
    setInvalid(false);
    save.reset();
    setDraft(rule);
  };
  const rules = blocklist.data?.rules ?? [];
  const currentPage = Math.min(page, Math.max(0, Math.ceil(rules.length / pageSize) - 1));

  return (
    <div className="mx-auto grid max-w-[96rem] gap-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="grid gap-1.5">
          <h1 className="text-2xl font-semibold">{t("blocklistTitle")}</h1>
          <p className="text-sm text-kumo-subtle">{t("blocklistIntro")}</p>
        </div>
        <Button variant="primary" onClick={() => edit(emptyDraft)}>{t("blocklistAdd")}</Button>
      </header>
      {blocklist.data && (
        <section className="grid gap-3 rounded-lg border border-kumo-line bg-kumo-elevated p-4">
          <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
            {(["hbbs", "hbbr"] as const).map((service) => (
              <div className="flex items-center gap-2 text-sm" key={service}>
                <span className="font-mono">{service}</span>
                <Badge>{t(syncLabels[blocklist.data[service]])}</Badge>
              </div>
            ))}
          </div>
          {!blocklist.data.paired_token_configured && (
            <Banner variant="alert" title={t("blocklistPairingRequired")} description={<span className="[overflow-wrap:anywhere]">{t("blocklistPairingHint")}</span>} />
          )}
          {blocklist.data.paired_token_configured && (blocklist.data.hbbs !== "synced" || blocklist.data.hbbr !== "synced") && (
            <p className="text-sm text-kumo-subtle">{t("blocklistSyncHint")}</p>
          )}
        </section>
      )}
      <section className="min-w-0 overflow-hidden rounded-lg border border-kumo-line bg-kumo-elevated">
        {blocklist.isPending && <TableState tone="loading">{t("loading")}</TableState>}
        {blocklist.error && <TableState tone="error">{blocklist.error.message}</TableState>}
        {blocklist.data && rules.length === 0 && <TableState tone="empty">{t("blocklistEmpty")}</TableState>}
        {rules.length > 0 && (
          <ul className="divide-y divide-kumo-line">
            {rules.slice(currentPage * pageSize, (currentPage + 1) * pageSize).map((rule) => (
              <li key={rule.id} className="flex flex-wrap items-center justify-between gap-2 px-4 py-3">
                <div className="grid min-w-0 gap-1">
                  <div className="flex flex-wrap items-center gap-2 text-sm">
                    <Badge>{t(kindLabels[rule.kind])}</Badge>
                    <span className="break-all font-mono font-medium">{rule.value}</span>
                  </div>
                  {rule.note && <span className="break-words text-sm text-kumo-subtle">{rule.note}</span>}
                </div>
                <div className="flex flex-wrap gap-1">
                  <Button size="sm" variant="ghost" className="max-sm:min-h-11" onClick={() => edit(rule)}>{t("edit")}</Button>
                  <Button size="sm" variant="ghost" className="max-sm:min-h-11" onClick={() => { remove.reset(); setRemoving(rule); }}>{t("delete")}</Button>
                </div>
              </li>
            ))}
          </ul>
        )}
        {rules.length > pageSize && (
          <div className="flex justify-end gap-2 border-t border-kumo-line p-4">
            <Button size="sm" variant="secondary" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>{t("bookPrevious")}</Button>
            <Button size="sm" variant="secondary" disabled={(currentPage + 1) * pageSize >= rules.length} onClick={() => setPage(currentPage + 1)}>{t("bookNext")}</Button>
          </div>
        )}
      </section>
      {draft && (
        <Dialog.Root open onOpenChange={(next) => { if (!next && !save.isPending) setDraft(undefined); }}>
          <Dialog size="sm" className={dialogPanelClass}>
            <DialogHeader title={t(draft.id ? "blocklistEdit" : "blocklistAdd")} />
            <DialogBody>
              <div className="grid gap-4">
                <Select<Kind> label={t("blocklistType")} value={draft.kind} onValueChange={(kind) => {
                  if (kind && kind !== draft.kind) { setInvalid(false); setDraft({ ...draft, kind, value: "" }); }
                }}>
                  {kinds.map((kind) => <Select.Option key={kind} value={kind}>{t(kindLabels[kind])}</Select.Option>)}
                </Select>
                <DevicePicker key={draft.kind} kind={draft.kind} onSelect={(value) => {
                  setInvalid(false);
                  setDraft({ ...draft, value });
                }} />
                <Input
                  label={t(valueLabels[draft.kind])}
                  placeholder={draft.kind === "ip" ? "192.0.2.10" : draft.kind === "cidr" ? "2001:db8::/48" : undefined}
                  autoComplete="off"
                  maxLength={128}
                  value={draft.value}
                  onChange={(event) => { setInvalid(false); setDraft({ ...draft, value: event.target.value }); }}
                  error={invalid ? t(invalidLabels[draft.kind]) : undefined}
                  description={t(kindHints[draft.kind])}
                />
                <Input label={t("blocklistNote")} maxLength={255} value={draft.note} onChange={(event) => setDraft({ ...draft, note: event.target.value })} />
              </div>
            </DialogBody>
            <DialogFooter error={save.error?.message}>
              <Button variant="secondary" onClick={() => setDraft(undefined)} disabled={save.isPending}>{t("cancel")}</Button>
              <Button variant="primary" loading={save.isPending} onClick={() => {
                if (isValidRule(draft)) save.mutate(draft);
                else setInvalid(true);
              }}>{t("save")}</Button>
            </DialogFooter>
          </Dialog>
        </Dialog.Root>
      )}
      {removing && <ConfirmDialog open title={t("blocklistRemove")} description={t("blocklistRemoveDescription", { value: removing.value })}
        confirmLabel={t("delete")} cancelLabel={t("cancel")} loading={remove.isPending} error={remove.error?.message}
        onConfirm={() => remove.mutate(removing.id)} onOpenChange={(open) => { if (!open && !remove.isPending) setRemoving(undefined); }} />}
    </div>
  );
}
