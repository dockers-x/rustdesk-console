import { useRef } from "react";
import { useMutation } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { Select } from "@cloudflare/kumo/components/select";
import { DownloadSimple } from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { InlineMessage } from "../components/InlineMessage";
import { apiGet } from "../lib/api";
import type { FilterDef } from "./types";

export function AuditExport({
  api,
  filename,
  params,
}: {
  api: string;
  filename: string;
  params: Record<string, string>;
}) {
  const { t } = useTranslation();
  const download = useMutation({
    mutationFn: async () => {
      const csv = await apiGet<string>(`${api}/export`, params);
      if (typeof csv !== "string") throw new Error(t("operationFailed"));
      // The browser can strip the BOM when decoding the API response as text.
      const utf8 = csv.startsWith("\uFEFF") ? csv : `\uFEFF${csv}`;
      const data = new Blob([utf8], { type: "text/csv;charset=utf-8" });
      const link = document.createElement("a");
      link.href = URL.createObjectURL(data);
      link.download = filename;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(link.href);
    },
  });

  return (
    <div className="mb-4 flex flex-wrap items-center gap-2">
      <Button
        type="button"
        variant="secondary"
        className="min-h-11 sm:min-h-0"
        icon={<DownloadSimple aria-hidden />}
        loading={download.isPending}
        disabled={download.isPending}
        onClick={() => download.mutate()}
        aria-describedby="audit-export-limit"
      >
        {t("auditExportCsv")}
      </Button>
      <p id="audit-export-limit" className="text-sm text-kumo-subtle">{t("auditExportLimitHint")}</p>
      {download.error && (
        <div className="w-full">
          <InlineMessage tone="error">{download.error.message || t("operationFailed")}</InlineMessage>
        </div>
      )}
    </div>
  );
}

export function ResourceFilters({
  definitions,
  filters,
  setFilter,
  clearFilters,
  timeZone,
}: {
  definitions: FilterDef[];
  filters: Record<string, string>;
  setFilter: (name: string, value: string) => void;
  clearFilters: () => void;
  timeZone?: string;
}) {
  const { t } = useTranslation();
  const panel = useRef<HTMLDivElement>(null);
  return (
    <div ref={panel} className="admin-filter-panel mb-4 rounded-xl border border-kumo-line p-4">
      <div className="flex flex-wrap items-end gap-3">
        {definitions.map((filter) => {
          const isWide = filter.name === "q" || filter.name === "path";
          const options = filter.type === "select"
            ? [
              { value: "all", label: t(filter.allLabel ?? "all") },
              ...(filter.options ?? []).map(({ label, value }) => ({
                value: String(value), label: t(label),
              })),
            ]
            : [];
          return (
            <div key={filter.name} className={isWide ? "min-w-0 basis-full sm:basis-60" : "min-w-0 basis-[calc(50%-0.375rem)] sm:basis-44"}>
              {filter.type === "select" ? (
                <Select<string>
                  label={t(filter.label)}
                  value={filters[filter.name] || "all"}
                  items={options}
                  onValueChange={(value) => setFilter(filter.name, value === "all" ? "" : value ?? "")}
                  className="min-h-11 w-full sm:min-h-0"
                >
                  {options.map(({ label, value }) => (
                    <Select.Option key={value} value={value}>{label}</Select.Option>
                  ))}
                </Select>
              ) : (
                <Input
                  label={t(filter.label)}
                  type={filter.type === "date" ? "date" : "text"}
                  className="min-h-11 min-w-0 w-full sm:min-h-0"
                  placeholder={filter.name === "q" ? t("auditKeywordPlaceholder") : undefined}
                  value={filters[filter.name] ?? ""}
                  onChange={(event) => setFilter(filter.name, event.target.value)}
                />
              )}
            </div>
          );
        })}
        {Object.values(filters).some(Boolean) && (
          <Button size="sm" variant="ghost" onClick={() => {
            clearFilters();
            panel.current?.querySelector<HTMLElement>('input:not([type="hidden"]), [role="combobox"]')?.focus();
          }}>{t("clearFilters")}</Button>
        )}
      </div>
      {timeZone && <p className="mt-2 text-sm text-kumo-subtle">{t("auditDateTimezone", { timeZone })}</p>}
    </div>
  );
}
