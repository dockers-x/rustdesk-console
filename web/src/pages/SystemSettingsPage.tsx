import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Input } from "@cloudflare/kumo/components/input";
import { GearSix, Info, NotePencil } from "@phosphor-icons/react";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPatch, ApiError } from "../lib/api";

interface AdminConfigView {
  title: string;
  hello: string;
  hello_raw: string;
  hello_file: string;
  timezone?: string;
}

interface AdminConfigForm {
  title: string;
  hello: string;
  hello_file: string;
}

const emptyForm: AdminConfigForm = {
  title: "",
  hello: "",
  hello_file: "",
};

function normalizeConfig(config: Partial<AdminConfigView>): AdminConfigForm {
  return {
    title: config.title ?? "",
    hello: config.hello_raw ?? "",
    hello_file: config.hello_file ?? "",
  };
}

export function SystemSettingsPage() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [form, setForm] = useState<AdminConfigForm>(emptyForm);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const config = useQuery({
    queryKey: ["admin-panel-config"],
    queryFn: () => apiGet<AdminConfigView>("/api/admin/config/admin/manage"),
    staleTime: 0,
    refetchOnMount: "always",
  });

  useEffect(() => {
    if (config.data) setForm(normalizeConfig(config.data));
  }, [config.data]);

  const save = useMutation({
    mutationFn: (payload: AdminConfigForm) =>
      apiPatch<AdminConfigView>("/api/admin/config/admin/manage", payload),
    onSuccess: (saved) => {
      setForm(normalizeConfig(saved));
      setMessage(t("siteWelcomeSaved"));
      setError("");
      void qc.invalidateQueries({ queryKey: ["admin-panel-config"] });
      void qc.invalidateQueries({ queryKey: ["public-admin-config"] });
    },
    onError: (err) => {
      const ae = err as ApiError;
      setError(ae.message || t("operationFailed"));
      setMessage("");
    },
  });

  const updateField = (field: keyof AdminConfigForm, value: string) => {
    setForm((current) => ({ ...current, [field]: value }));
    setMessage("");
    setError("");
  };

  const inlineHello = form.hello.trim();
  const filePath = form.hello_file.trim();
  const canReuseFilePreview =
    filePath &&
    filePath === config.data?.hello_file?.trim() &&
    !config.data?.hello_raw?.trim() &&
    config.data?.hello;
  const preview = inlineHello
    ? form.hello.split("{{username}}").join(t("username"))
    : canReuseFilePreview
      ? (config.data?.hello ?? "")
      : filePath
        ? t("siteWelcomeFilePreviewPending")
        : "";

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("systemSettings")}</h1>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">
            {t("systemSettingsHint")}
          </p>
        </div>
        <Button
          loading={save.isPending}
          onClick={() =>
            save.mutate({
              title: form.title.trim(),
              hello: form.hello.trim(),
              hello_file: form.hello_file.trim(),
            })
          }
        >
          <GearSix size={16} />
          {t("writeSystemSettings")}
        </Button>
      </div>

      {message && <InlineMessage tone="success">{message}</InlineMessage>}
      {error && <InlineMessage tone="error">{error}</InlineMessage>}

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
        <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
          <div className="mb-5 flex items-start gap-3">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
              <NotePencil size={18} />
            </div>
            <div>
              <h2 className="text-base font-semibold">{t("siteIdentity")}</h2>
              <p className="mt-1 text-sm text-kumo-subtle">
                {t("siteIdentityHint")}
              </p>
            </div>
          </div>

          {config.isLoading && <TableState tone="loading">{t("loading")}</TableState>}
          {config.error && (
            <TableState tone="error">
              {(config.error as Error).message || t("operationFailed")}
            </TableState>
          )}

          {!config.isLoading && !config.error && (
            <div className="grid gap-5">
              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("siteTitle")}
                </span>
                <Input
                  aria-label={t("siteTitle")}
                  value={form.title}
                  maxLength={80}
                  onChange={(e) => updateField("title", e.target.value)}
                />
                <span className="mt-1.5 block text-xs text-kumo-subtle">
                  {t("siteTitleHint")}
                </span>
              </label>

              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("siteWelcomeFile")}
                </span>
                <Input
                  aria-label={t("siteWelcomeFile")}
                  value={form.hello_file}
                  maxLength={500}
                  placeholder="/etc/rustdesk-console/welcome.txt"
                  onChange={(e) => updateField("hello_file", e.target.value)}
                />
                <span className="mt-1.5 block text-xs text-kumo-subtle">
                  {t("siteWelcomeFileHint")}
                </span>
              </label>

              <label className="block">
                <span className="mb-1.5 block text-sm font-medium">
                  {t("siteWelcomeInline")}
                </span>
                <textarea
                  className="min-h-40 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm leading-6 focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
                  aria-label={t("siteWelcomeInline")}
                  value={form.hello}
                  maxLength={5000}
                  onChange={(e) => updateField("hello", e.target.value)}
                />
                <span className="mt-1.5 block text-xs text-kumo-subtle">
                  {t("siteWelcomeInlineHint")}
                </span>
              </label>
            </div>
          )}
        </section>

        <aside className="space-y-4">
          <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
            <div className="mb-4 flex items-start gap-3">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-kumo-line bg-kumo-base text-kumo-brand">
                <Info size={18} />
              </div>
              <div>
                <h2 className="text-base font-semibold">
                  {t("siteWelcomePreview")}
                </h2>
                <p className="mt-1 text-sm text-kumo-subtle">
                  {t("siteWelcomePreviewHint")}
                </p>
              </div>
            </div>
            <div className="min-h-32 whitespace-pre-wrap break-words rounded-lg border border-kumo-line bg-kumo-base px-3 py-2 text-sm leading-6">
              {preview || t("emptyValue")}
            </div>
          </section>

          <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-5">
            <h2 className="text-base font-semibold">{t("siteWelcome")}</h2>
            <p className="mt-2 text-sm leading-6 text-kumo-subtle">
              {t("siteWelcomeHint")}
            </p>
            <p className="mt-3 rounded-md border border-kumo-line bg-kumo-base px-3 py-2 text-xs leading-5 text-kumo-subtle">
              {t("siteWelcomeFilePriority")}
            </p>
          </section>
        </aside>
      </div>
    </div>
  );
}
