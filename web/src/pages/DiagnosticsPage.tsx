import { useMutation } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Table } from "@cloudflare/kumo/components/table";
import {
  CheckCircle,
  Pulse,
  WarningCircle,
  XCircle,
} from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { cn } from "@cloudflare/kumo/utils";
import { TableState } from "../components/TableState";
import { usePublicAdminConfig } from "../lib/adminTitle";
import { apiPost } from "../lib/api";
import { formatDateTime } from "../lib/dateFormat";

type DiagnosticStatus = "ok" | "warning" | "error";

interface DiagnosticsReport {
  generated_at: string;
  summary: {
    ok: number;
    warning: number;
    error: number;
  };
  checks: {
    key: string;
    label: string;
    status: DiagnosticStatus;
    message: string;
    elapsed_ms: number;
  }[];
}

export function DiagnosticsPage() {
  const { t } = useTranslation();
  const adminConfig = usePublicAdminConfig();
  const displayTimeZone = adminConfig.data?.timezone?.trim() || undefined;
  const diagnostics = useMutation({
    mutationFn: () => apiPost<DiagnosticsReport>("/api/admin/diagnostics/run"),
  });
  const data = diagnostics.data;

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("diagnostics")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("diagnosticsHint")}
          </p>
        </div>
        <Button
          onClick={() => diagnostics.mutate()}
          loading={diagnostics.isPending}
        >
          {t("runDiagnostics")}
        </Button>
      </div>

      <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
        <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div className="flex items-start gap-3">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
              <Pulse size={18} />
            </div>
            <div>
              <h2 className="text-base font-semibold">{t("runtimeChecks")}</h2>
              <p className="mt-1 text-sm text-kumo-subtle">
                {data
                  ? t("diagnosticsGenerated", {
                      value: formatDateTime(data.generated_at, displayTimeZone),
                    })
                  : t("diagnosticsEmptyHint")}
              </p>
            </div>
          </div>
          {data && (
            <div className="flex flex-wrap gap-2">
              <Badge>{t("ok")}: {data.summary.ok}</Badge>
              <Badge>{t("warning")}: {data.summary.warning}</Badge>
              <Badge>{t("error")}: {data.summary.error}</Badge>
            </div>
          )}
        </div>

        {!data && !diagnostics.isPending && !diagnostics.error && (
          <TableState tone="empty">{t("diagnosticsEmptyState")}</TableState>
        )}
        {diagnostics.isPending && (
          <TableState tone="loading">{t("loading")}</TableState>
        )}
        {diagnostics.error && (
          <TableState tone="error">
            {(diagnostics.error as Error).message || t("operationFailed")}
          </TableState>
        )}
        {data && (
          <div className="overflow-x-auto rounded-lg border border-kumo-line">
            <Table>
              <Table.Header>
                <Table.Row>
                  <Table.Head>{t("status")}</Table.Head>
                  <Table.Head>{t("checkItem")}</Table.Head>
                  <Table.Head>{t("message")}</Table.Head>
                  <Table.Head>{t("elapsed")}</Table.Head>
                </Table.Row>
              </Table.Header>
              <Table.Body>
                {data.checks.map((check) => (
                  <Table.Row key={check.key}>
                    <Table.Cell>
                      <StatusPill status={check.status} />
                    </Table.Cell>
                    <Table.Cell className="font-medium">{check.label}</Table.Cell>
                    <Table.Cell>
                      <span className="block min-w-72 max-w-3xl whitespace-normal text-sm">
                        {check.message}
                      </span>
                    </Table.Cell>
                    <Table.Cell className="font-mono text-xs tabular-nums">
                      {check.elapsed_ms}ms
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table>
          </div>
        )}
      </section>
    </div>
  );
}

function StatusPill({ status }: { status: DiagnosticStatus }) {
  const { t } = useTranslation();
  const Icon =
    status === "ok" ? CheckCircle : status === "warning" ? WarningCircle : XCircle;
  return (
    <span
      className={cn(
        "inline-flex min-h-7 items-center gap-1.5 rounded-md border px-2.5 text-xs font-medium",
        status === "ok" &&
          "border-kumo-success/25 bg-kumo-success-tint/60 text-kumo-success",
        status === "warning" &&
          "border-kumo-warning/25 bg-kumo-warning-tint/60 text-kumo-warning",
        status === "error" &&
          "border-kumo-danger/25 bg-kumo-danger-tint/40 text-kumo-danger",
      )}
    >
      <Icon size={14} weight={status === "ok" ? "fill" : "regular"} />
      {t(status)}
    </span>
  );
}
