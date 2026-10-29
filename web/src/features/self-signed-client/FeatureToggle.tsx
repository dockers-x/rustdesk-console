import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Switch } from "@cloudflare/kumo/components/switch";
import { ArrowRight, ShieldCheck } from "@phosphor-icons/react";
import { InlineMessage } from "../../components/InlineMessage";
import { TableState } from "../../components/TableState";
import { apiPut } from "../../lib/api";
import {
  featureError,
  featuresQueryKey,
  selfSignedBase,
  selfSignedEnabled,
  selfSignedSettingsKey,
  useFeatures,
  type Features,
  type SigningSettings,
} from "./api";

export function SelfSignedClientFeatureToggle() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const features = useFeatures();
  const enabled = selfSignedEnabled(features.data);
  const save = useMutation({
    mutationFn: (next: boolean) => apiPut<SigningSettings>(selfSignedBase, { enabled: next }),
    onSuccess: (settings) => {
      queryClient.setQueryData<Features>(featuresQueryKey, {
        self_signed_client: { enabled: settings.enabled },
      });
      queryClient.setQueryData(selfSignedSettingsKey, settings);
    },
  });

  return (
    <section className="rounded-lg border border-kumo-line bg-kumo-elevated p-4 sm:p-5">
      <div className="flex items-start gap-3">
        <ShieldCheck size={24} className="mt-0.5 shrink-0 text-kumo-brand" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <h2 className="text-base font-semibold">{t("selfSignedClients")}</h2>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-kumo-subtle">{t("selfSignedFeatureHint")}</p>
        </div>
      </div>
      {features.isPending ? <TableState tone="loading">{t("loading")}</TableState> : features.isError ? (
        <TableState tone="error">{featureError(features.error)}</TableState>
      ) : (
        <div className="mt-5 space-y-4">
          <Switch
            checked={enabled}
            disabled={save.isPending}
            onCheckedChange={(next) => save.mutate(next)}
            label={t("selfSignedEnable")}
          />
          <p className="text-sm leading-6 text-kumo-subtle">{t("selfSignedEnableHint")}</p>
          {enabled && (
            <Link to="/self-signed-client" className="inline-flex min-h-11 items-center gap-2 text-sm font-medium text-kumo-brand underline-offset-4 hover:underline">
              {t("selfSignedManage")}<ArrowRight size={16} aria-hidden="true" />
            </Link>
          )}
          <p className="text-xs leading-5 text-kumo-subtle">{t("selfSignedDisableHint")}</p>
        </div>
      )}
      {save.isError && <div className="mt-4"><InlineMessage tone="error">{featureError(save.error)}</InlineMessage></div>}
    </section>
  );
}
