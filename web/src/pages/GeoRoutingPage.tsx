import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, Database, Plus } from "@phosphor-icons/react";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input } from "@cloudflare/kumo/components/input";
import { Table } from "@cloudflare/kumo/components/table";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPost, apiPut } from "../lib/api";

type MmdbKind = "country" | "city" | "asn";

interface GeoRule {
  name: string;
  symmetric: boolean;
  match: { clientA: string; clientB: string };
  relays: string[];
}

interface GeoSettings {
  version: number;
  revision: number;
  enabled: boolean;
  rules: GeoRule[];
}

interface MmdbStatus {
  kind: MmdbKind;
  isPresent: boolean;
  sizeBytes?: number | null;
  modifiedAt?: string | null;
  databaseType?: string | null;
  buildEpoch?: number | null;
  hasBackup: boolean;
  sourceUrl?: string | null;
  error?: string | null;
}

interface RuntimeStatus {
  isAvailable: boolean;
  appliedRevision?: number | null;
  isGeoEnabled?: boolean | null;
  ruleCount?: number | null;
  countryAvailable?: boolean | null;
  cityAvailable?: boolean | null;
  asnAvailable?: boolean | null;
  warnings: string[];
  error?: string | null;
}

interface GeoOverview {
  settings: GeoSettings;
  databases: MmdbStatus[];
  updatePolicy: MmdbUpdatePolicy;
  runtime: RuntimeStatus;
}

interface RelayPreview {
  revision: number;
  reason: "rule_match" | "disabled" | "no_eligible_match" | "no_relay";
  relay: string | null;
  rule: string | null;
  candidates: string[];
  warnings: string[];
}

const previewReasonLabel = {
  rule_match: "geoPreviewRuleMatch",
  disabled: "geoPreviewDisabled",
  no_eligible_match: "geoPreviewNoMatch",
  no_relay: "geoPreviewNoRelay",
} as const;

interface MmdbUpdatePolicy {
  enabled: boolean;
  intervalHours: number;
}

interface SaveSettingsResult {
  settings: GeoSettings;
  isPersisted: boolean;
  isApplied: boolean;
  applyMessage?: string | null;
}

interface DatabaseActionResult {
  database: MmdbStatus;
  isReloaded: boolean;
  reloadMessage?: string | null;
  isSourceSaved?: boolean;
  sourceMessage?: string | null;
}

interface RuleDraft {
  name: string;
  symmetric: boolean;
  clientA: string;
  clientB: string;
  relays: string;
}

const emptyRule: RuleDraft = {
  name: "",
  symmetric: true,
  clientA: "*",
  clientB: "*",
  relays: "",
};

function formatBytes(value?: number | null) {
  if (value == null) return "-";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function relayLines(value: string) {
  return value
    .split(/[\n,;]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function GeoRoutingPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [settings, setSettings] = useState<GeoSettings | null>(null);
  const synchronizedSettings = useRef<GeoSettings | null>(null);
  const [updatePolicy, setUpdatePolicy] = useState<MmdbUpdatePolicy | null>(null);
  const synchronizedUpdatePolicy = useRef<MmdbUpdatePolicy | null>(null);
  const [ruleOpen, setRuleOpen] = useState(false);
  const [ruleIndex, setRuleIndex] = useState<number | null>(null);
  const [rule, setRule] = useState<RuleDraft>(emptyRule);
  const [deleteIndex, setDeleteIndex] = useState<number | null>(null);
  const [databaseKind, setDatabaseKind] = useState<MmdbKind | null>(null);
  const [sourceUrl, setSourceUrl] = useState("");
  const [restoreKind, setRestoreKind] = useState<MmdbKind | null>(null);
  const [clientA, setClientA] = useState("");
  const [clientB, setClientB] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const overview = useQuery({
    queryKey: ["rustdesk-geo"],
    queryFn: () => apiGet<GeoOverview>("/api/admin/rustdesk/geo"),
  });

  useEffect(() => {
    if (!overview.data) return;
    const incoming = overview.data.settings;
    setSettings((current) => {
      const baseline = synchronizedSettings.current;
      const hasLocalChanges =
        current != null && baseline != null && JSON.stringify(current) !== JSON.stringify(baseline);
      synchronizedSettings.current = incoming;
      return hasLocalChanges ? current : incoming;
    });
    const incomingPolicy = overview.data.updatePolicy;
    setUpdatePolicy((current) => {
      const baseline = synchronizedUpdatePolicy.current;
      const hasLocalChanges =
        current != null && baseline != null && JSON.stringify(current) !== JSON.stringify(baseline);
      synchronizedUpdatePolicy.current = incomingPolicy;
      return hasLocalChanges ? current : incomingPolicy;
    });
  }, [overview.data]);

  const refresh = () => qc.invalidateQueries({ queryKey: ["rustdesk-geo"] });

  const save = useMutation({
    mutationFn: async () => {
      const submitted = settings;
      const result = await apiPut<SaveSettingsResult>("/api/admin/rustdesk/geo/settings", submitted);
      return { result, submitted };
    },
    onSuccess: ({ result, submitted }) => {
      synchronizedSettings.current = result.settings;
      setSettings((current) => {
        if (JSON.stringify(current) === JSON.stringify(submitted)) return result.settings;
        return current
          ? { ...current, version: result.settings.version, revision: result.settings.revision }
          : current;
      });
      setError("");
      setMessage(
        result.isApplied
          ? t("geoSavedApplied")
          : `${t("geoSavedNotApplied")}${result.applyMessage ? `: ${result.applyMessage}` : ""}`,
      );
      void refresh();
    },
    onError: (value) => {
      setMessage("");
      setError((value as Error).message || t("operationFailed"));
    },
  });

  const download = useMutation({
    mutationFn: () =>
      apiPost<DatabaseActionResult>(
        `/api/admin/rustdesk/geo/databases/${databaseKind}/download`,
        { sourceUrl: sourceUrl.trim() },
      ),
    onSuccess: (result) => {
      setDatabaseKind(null);
      setError("");
      const installMessage = result.isReloaded
        ? t("geoMmdbInstalled")
        : `${t("geoMmdbInstalledNotReloaded")}${result.reloadMessage ? `: ${result.reloadMessage}` : ""}`;
      setMessage(installMessage);
      if (result.isSourceSaved === false) {
        setError(result.sourceMessage || t("geoMmdbSourceNotSaved"));
      }
      void refresh();
    },
    onError: (value) => setError((value as Error).message || t("operationFailed")),
  });

  const restore = useMutation({
    mutationFn: (kind: MmdbKind) =>
      apiPost<DatabaseActionResult>(`/api/admin/rustdesk/geo/databases/${kind}/restore`),
    onSuccess: (result) => {
      setRestoreKind(null);
      setError("");
      setMessage(
        result.isReloaded
          ? t("geoMmdbRestored")
          : `${t("geoMmdbRestoredNotReloaded")}${result.reloadMessage ? `: ${result.reloadMessage}` : ""}`,
      );
      void refresh();
    },
    onError: (value) => setError((value as Error).message || t("operationFailed")),
  });

  const saveUpdatePolicy = useMutation({
    mutationFn: async () => {
      const submitted = updatePolicy;
      const result = await apiPut<MmdbUpdatePolicy>(
        "/api/admin/rustdesk/geo/databases/update-policy",
        submitted,
      );
      return { result, submitted };
    },
    onSuccess: ({ result, submitted }) => {
      synchronizedUpdatePolicy.current = result;
      setUpdatePolicy((current) =>
        JSON.stringify(current) === JSON.stringify(submitted) ? result : current,
      );
      setError("");
      setMessage(t("geoAutoUpdateSaved"));
      void refresh();
    },
    onError: (value) => setError((value as Error).message || t("operationFailed")),
  });

  const reload = useMutation({
    mutationFn: () => apiPost<{ isApplied: boolean; message: string }>("/api/admin/rustdesk/geo/reload"),
    onSuccess: (result) => {
      setError(result.isApplied ? "" : result.message);
      setMessage(result.isApplied ? result.message || t("geoReloaded") : "");
      void refresh();
    },
    onError: (value) => setError((value as Error).message || t("operationFailed")),
  });

  const test = useMutation({
    mutationFn: () =>
      apiPost<{ preview: RelayPreview | null }>("/api/admin/rustdesk/geo/preview", {
        clientA: clientA.trim(),
        clientB: clientB.trim() || null,
      }),
  });

  const openRule = (index?: number) => {
    if (index == null || !settings) {
      setRuleIndex(null);
      setRule(emptyRule);
    } else {
      const current = settings.rules[index];
      setRuleIndex(index);
      setRule({
        name: current.name,
        symmetric: current.symmetric,
        clientA: current.match.clientA,
        clientB: current.match.clientB,
        relays: current.relays.join("\n"),
      });
    }
    setRuleOpen(true);
  };

  const commitRule = () => {
    if (!settings) return;
    const next: GeoRule = {
      name: rule.name.trim(),
      symmetric: rule.symmetric,
      match: {
        clientA: rule.clientA.trim() || "*",
        clientB: rule.clientB.trim() || "*",
      },
      relays: relayLines(rule.relays),
    };
    const rules = [...settings.rules];
    if (ruleIndex == null) rules.push(next);
    else rules[ruleIndex] = next;
    setSettings({ ...settings, rules });
    setRuleOpen(false);
  };

  const moveRule = (index: number, offset: -1 | 1) => {
    if (!settings) return;
    const target = index + offset;
    if (target < 0 || target >= settings.rules.length) return;
    const rules = [...settings.rules];
    [rules[index], rules[target]] = [rules[target], rules[index]];
    setSettings({ ...settings, rules });
  };

  const runtime = overview.data?.runtime;
  const dirty = settings != null && JSON.stringify(settings) !== JSON.stringify(overview.data?.settings);
  const updatePolicyDirty =
    updatePolicy != null && JSON.stringify(updatePolicy) !== JSON.stringify(overview.data?.updatePolicy);

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h1 className="text-xl font-semibold">{t("geoRouting")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-default/75">{t("geoRoutingHint")}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" loading={overview.isFetching} onClick={() => void overview.refetch()}>
            {t("refresh")}
          </Button>
          <Button variant="secondary" loading={reload.isPending} onClick={() => reload.mutate()}>
            {t("geoReload")}
          </Button>
          <Button disabled={!settings || (!dirty && settings.revision > 0)} loading={save.isPending} onClick={() => save.mutate()}>
            {t("save")}
          </Button>
        </div>
      </div>

      {message && <InlineMessage tone="success">{message}</InlineMessage>}
      {error && <InlineMessage tone="error">{error}</InlineMessage>}
      {overview.error && <InlineMessage tone="error">{(overview.error as Error).message}</InlineMessage>}

      <section className="rounded-lg border border-kumo-line bg-kumo-base p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <h2 className="text-base font-semibold">{t("geoRuntime")}</h2>
            <p className="mt-1 text-sm text-kumo-default/75">{t("geoRuntimeHint")}</p>
          </div>
          <Badge variant={runtime?.isAvailable ? "success" : "secondary"}>
            {runtime?.isAvailable ? t("geoHbbsAvailable") : t("geoHbbsUnavailable")}
          </Badge>
        </div>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Status label={t("geoSavedRevision")} value={settings?.revision ?? "-"} />
          <Status label={t("geoAppliedRevision")} value={runtime?.appliedRevision ?? "-"} />
          <Status label={t("geoRuntimeRules")} value={runtime?.ruleCount ?? "-"} />
          <Status
            label={t("geoRuntimeState")}
            value={runtime?.isGeoEnabled == null ? "-" : runtime.isGeoEnabled ? t("enabled") : t("disabled")}
          />
        </div>
        {(Boolean(runtime?.error) || (runtime?.warnings.length ?? 0) > 0) && (
          <p className="mt-3 break-words text-sm text-kumo-default/75">
            {[runtime?.error, ...(runtime?.warnings ?? [])].filter(Boolean).join(" · ")}
          </p>
        )}
      </section>

      <section>
        <div className="mb-3 flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h2 className="text-base font-semibold">{t("geoMmdbTitle")}</h2>
            <p className="mt-1 text-sm text-kumo-default/75">{t("geoMmdbHint")}</p>
          </div>
          <div className="flex flex-wrap items-end gap-3 rounded-lg border border-kumo-line bg-kumo-base p-3">
            <label className="inline-flex h-9 items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={updatePolicy?.enabled ?? false}
                onChange={(event) => updatePolicy && setUpdatePolicy({ ...updatePolicy, enabled: event.target.checked })}
              />
              {t("geoAutoUpdate")}
            </label>
            <Field label={t("geoUpdateIntervalHours")}>
              <Input
                className="w-28"
                type="number"
                min={6}
                max={720}
                value={updatePolicy?.intervalHours ?? 168}
                onChange={(event) => updatePolicy && setUpdatePolicy({ ...updatePolicy, intervalHours: Number(event.target.value) })}
              />
            </Field>
            <Button
              size="sm"
              disabled={!updatePolicy || !updatePolicyDirty}
              loading={saveUpdatePolicy.isPending}
              onClick={() => saveUpdatePolicy.mutate()}
            >
              {t("save")}
            </Button>
          </div>
        </div>
        <div className="grid gap-3 lg:grid-cols-3">
          {(overview.data?.databases ?? []).map((database) => {
            const readerAvailable = runtime?.[`${database.kind}Available` as keyof RuntimeStatus] === true;
            return (
              <article key={database.kind} className="rounded-lg border border-kumo-line bg-kumo-base p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 items-center gap-3">
                    <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-kumo-recessed text-kumo-subtle">
                      <Database size={19} />
                    </div>
                    <div className="min-w-0">
                      <h3 className="font-semibold">{t(`geoMmdb_${database.kind}`)}</h3>
                      <p className="truncate text-xs text-kumo-default/75">{database.databaseType || t("geoMmdbMissing")}</p>
                    </div>
                  </div>
                  <Badge variant={database.isPresent && readerAvailable && !database.error ? "success" : "secondary"}>
                    {database.isPresent && readerAvailable && !database.error ? t("loaded") : database.isPresent ? t("geoInstalled") : t("geoOptional")}
                  </Badge>
                </div>
                <dl className="mt-4 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-2 text-xs">
                  <dt className="text-kumo-default/75">{t("geoMmdbSize")}</dt><dd className="text-right">{formatBytes(database.sizeBytes)}</dd>
                  <dt className="text-kumo-default/75">{t("geoMmdbModified")}</dt><dd className="break-all text-right">{database.modifiedAt || "-"}</dd>
                  <dt className="text-kumo-default/75">{t("geoMmdbBackup")}</dt><dd className="text-right">{database.hasBackup ? t("available") : t("noData")}</dd>
                </dl>
                {database.error && <p className="mt-3 break-words text-xs text-kumo-danger">{database.error}</p>}
                <div className="mt-4 flex flex-wrap gap-2">
                  <Button size="sm" onClick={() => { setDatabaseKind(database.kind); setSourceUrl(database.sourceUrl || ""); }}>
                    {t("download")}
                  </Button>
                  <Button size="sm" variant="secondary" disabled={!database.hasBackup} onClick={() => setRestoreKind(database.kind)}>
                    {t("geoRestoreBackup")}
                  </Button>
                </div>
              </article>
            );
          })}
        </div>
      </section>

      <section className="rounded-lg border border-kumo-line bg-kumo-base">
        <div className="flex flex-col gap-3 border-b border-kumo-line p-4 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <div className="flex items-center gap-3">
              <h2 className="text-base font-semibold">{t("geoRules")}</h2>
              <label className="inline-flex items-center gap-2 text-sm">
                <input type="checkbox" checked={settings?.enabled ?? false} onChange={(event) => settings && setSettings({ ...settings, enabled: event.target.checked })} />
                {t("enabled")}
              </label>
            </div>
            <p className="mt-1 text-sm text-kumo-default/75">{t("geoRulesHint")}</p>
          </div>
          <Button size="sm" onClick={() => openRule()}><Plus size={16} />{t("create")}</Button>
        </div>
        <div className="overflow-x-auto">
          <Table>
            <Table.Header><Table.Row>
              <Table.Head>#</Table.Head><Table.Head>{t("name")}</Table.Head><Table.Head>{t("geoClientA")}</Table.Head><Table.Head>{t("geoClientB")}</Table.Head><Table.Head>{t("geoRelays")}</Table.Head><Table.Head>{t("actions")}</Table.Head>
            </Table.Row></Table.Header>
            <Table.Body>{(settings?.rules ?? []).map((item, index) => (
              <Table.Row key={`${item.name}-${index}`}>
                <Table.Cell>{index + 1}</Table.Cell>
                <Table.Cell><div className="font-medium">{item.name}</div><div className="text-xs text-kumo-default/75">{item.symmetric ? t("geoSymmetric") : t("geoDirectional")}</div></Table.Cell>
                <Table.Cell><code className="whitespace-nowrap text-xs">{item.match.clientA}</code></Table.Cell>
                <Table.Cell><code className="whitespace-nowrap text-xs">{item.match.clientB}</code></Table.Cell>
                <Table.Cell><div className="max-w-72 break-words text-xs">{item.relays.join(", ")}</div></Table.Cell>
                <Table.Cell><div className="flex min-w-max flex-wrap gap-1">
                  <Button size="sm" variant="ghost" aria-label={t("geoMoveUp")} disabled={index === 0} onClick={() => moveRule(index, -1)}><ArrowUp size={16} /></Button>
                  <Button size="sm" variant="ghost" aria-label={t("geoMoveDown")} disabled={index === (settings?.rules.length ?? 0) - 1} onClick={() => moveRule(index, 1)}><ArrowDown size={16} /></Button>
                  <Button size="sm" variant="ghost" onClick={() => openRule(index)}>{t("edit")}</Button>
                  <Button size="sm" variant="secondary-destructive" onClick={() => setDeleteIndex(index)}>{t("delete")}</Button>
                </div></Table.Cell>
              </Table.Row>
            ))}</Table.Body>
          </Table>
          {!overview.isLoading && (settings?.rules.length ?? 0) === 0 && <TableState tone="empty">{t("geoNoRules")}</TableState>}
          {overview.isLoading && <TableState tone="loading">{t("loading")}</TableState>}
        </div>
      </section>

      <section className="rounded-lg border border-kumo-line bg-kumo-base p-4">
        <h2 className="text-base font-semibold">{t("geoTestTitle")}</h2>
        <p className="mt-1 text-sm text-kumo-default/75">{t("geoTestHint")}</p>
        <form
          className="mt-4 grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-end"
          onSubmit={(event) => {
            event.preventDefault();
            if (clientA.trim() && !test.isPending) test.mutate();
          }}
        >
          <Input
            label={t("geoClientA")}
            value={clientA}
            placeholder="203.0.113.10"
            maxLength={45}
            disabled={test.isPending}
            onChange={(event) => { setClientA(event.target.value); test.reset(); }}
          />
          <Input
            label={t("geoClientBOptional")}
            value={clientB}
            placeholder="198.51.100.20"
            maxLength={45}
            disabled={test.isPending}
            onChange={(event) => { setClientB(event.target.value); test.reset(); }}
          />
          <Button type="submit" disabled={!clientA.trim()} loading={test.isPending}>{t("geoRunTest")}</Button>
        </form>
        {test.isError && <InlineMessage tone="error" className="mt-3">{test.error.message || t("operationFailed")}</InlineMessage>}
        {test.isSuccess && (
          test.data.preview
            ? <RelayPreviewResult preview={test.data.preview} />
            : <InlineMessage tone="error" className="mt-3">{t("geoPreviewUnavailable")}</InlineMessage>
        )}
      </section>

      <Dialog.Root open={ruleOpen} onOpenChange={setRuleOpen}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader title={ruleIndex == null ? t("geoCreateRule") : t("geoEditRule")} description={<span className="text-kumo-default/75">{t("geoRuleDialogHint")}</span>} />
          <DialogBody><div className="grid gap-4">
            <Field label={t("name")}><Input maxLength={120} value={rule.name} onChange={(event) => setRule({ ...rule, name: event.target.value })} /></Field>
            <label className="inline-flex items-center gap-2 text-sm"><input type="checkbox" checked={rule.symmetric} onChange={(event) => setRule({ ...rule, symmetric: event.target.checked })} />{t("geoSymmetricHint")}</label>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label={t("geoClientA")}><Input maxLength={512} value={rule.clientA} onChange={(event) => setRule({ ...rule, clientA: event.target.value })} /></Field>
              <Field label={t("geoClientB")}><Input maxLength={512} value={rule.clientB} onChange={(event) => setRule({ ...rule, clientB: event.target.value })} /></Field>
            </div>
            <Field label={t("geoRelays")} hint={t("geoRelaysHint")}><textarea className="min-h-24 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 font-mono text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand sm:min-h-32" value={rule.relays} onChange={(event) => setRule({ ...rule, relays: event.target.value })} /></Field>
            <p className="text-xs leading-5 text-kumo-default/75">{t("geoExpressionHint")}</p>
          </div></DialogBody>
          <DialogFooter><Button variant="secondary" onClick={() => setRuleOpen(false)}>{t("cancel")}</Button><Button disabled={!rule.name.trim() || relayLines(rule.relays).length === 0} onClick={commitRule}>{t("save")}</Button></DialogFooter>
        </Dialog>
      </Dialog.Root>

      <Dialog.Root open={databaseKind != null} onOpenChange={(open) => !open && setDatabaseKind(null)}>
        <Dialog size="lg" className={dialogPanelClass}>
          <DialogHeader title={`${t("download")} MMDB`} description={<span className="text-kumo-default/75">{t("geoDownloadHint")}</span>} />
          <DialogBody><Field label={t("geoSourceUrl")}><Input maxLength={4096} value={sourceUrl} placeholder="https://example.com/GeoLite2-City.mmdb" onChange={(event) => setSourceUrl(event.target.value)} /></Field></DialogBody>
          <DialogFooter error={download.error ? (download.error as Error).message : undefined}><Button variant="secondary" onClick={() => setDatabaseKind(null)}>{t("cancel")}</Button><Button disabled={!sourceUrl.trim()} loading={download.isPending} onClick={() => download.mutate()}>{t("download")}</Button></DialogFooter>
        </Dialog>
      </Dialog.Root>

      <ConfirmDialog open={deleteIndex != null} title={t("geoDeleteRule")} description={t("geoDeleteRuleHint")} confirmLabel={t("delete")} cancelLabel={t("cancel")} onOpenChange={(open) => !open && setDeleteIndex(null)} onConfirm={() => { if (settings && deleteIndex != null) setSettings({ ...settings, rules: settings.rules.filter((_, index) => index !== deleteIndex) }); setDeleteIndex(null); }} />
      <ConfirmDialog open={restoreKind != null} title={t("geoRestoreBackup")} description={t("geoRestoreBackupHint")} confirmLabel={t("geoRestoreBackup")} cancelLabel={t("cancel")} loading={restore.isPending} error={restore.error ? (restore.error as Error).message : undefined} onOpenChange={(open) => !open && setRestoreKind(null)} onConfirm={() => restoreKind && restore.mutate(restoreKind)} />
    </div>
  );
}

function Status({ label, value }: { label: string; value: string | number }) {
  return <div className="rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-3"><div className="text-xs text-kumo-default/75">{label}</div><div className="mt-1 break-words font-mono text-sm font-semibold">{value}</div></div>;
}

function RelayPreviewResult({ preview }: { preview: RelayPreview }) {
  const { t } = useTranslation();
  return (
    <div role="status" className="mt-4 space-y-3 rounded-md border border-kumo-line bg-kumo-elevated p-3 text-sm">
      <p>{t(previewReasonLabel[preview.reason])}</p>
      <dl className="grid gap-3 sm:grid-cols-3">
        <div><dt className="text-kumo-subtle">{t("geoPreviewRevision")}</dt><dd>{preview.revision}</dd></div>
        {preview.rule && <div><dt className="text-kumo-subtle">{t("geoPreviewMatchedRule")}</dt><dd className="break-words">{preview.rule}</dd></div>}
        {preview.relay && <div><dt className="text-kumo-subtle">{t("geoPreviewRelay")}</dt><dd className="break-words">{preview.relay}</dd></div>}
      </dl>
      {preview.candidates.length > 0 && (
        <div>
          <p className="text-kumo-subtle">{t("geoPreviewCandidates")}</p>
          <ul className="mt-1 flex flex-wrap gap-1.5">
            {[...new Set(preview.candidates)].map((relay) => (
              <li key={relay} className="min-w-0">
                <Badge className="max-w-full whitespace-normal break-all">{relay}</Badge>
              </li>
            ))}
          </ul>
        </div>
      )}
      {!preview.relay && preview.candidates.length > 1 && <p>{t("geoPreviewRotation")}</p>}
      {preview.warnings.length > 0 && (
        <ul className="list-disc space-y-1 pl-5 text-kumo-subtle">
          {preview.warnings.map((warning) => <li key={warning} className="break-words">{warning}</li>)}
        </ul>
      )}
    </div>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return <label className="block"><span className="mb-1 block text-sm">{label}</span>{children}{hint && <span className="mt-1 block text-xs leading-5 text-kumo-default/75">{hint}</span>}</label>;
}
