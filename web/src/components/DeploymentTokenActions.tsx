import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { Prohibit } from "@phosphor-icons/react";
import { apiPost } from "../lib/api";

export function DeploymentTokenActions({ id }: { id: number }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const revoke = async () => {
    if (!id || loading) return;
    setLoading(true);
    setError("");
    try {
      await apiPost("/api/admin/deployment_token/revoke", { id });
      await qc.invalidateQueries({ queryKey: ["deployment_tokens"] });
    } catch (err) {
      setError((err as Error).message || t("operationFailed"));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="inline-flex flex-col items-start gap-1">
      <Button
        type="button"
        size="sm"
        variant="secondary"
        aria-label={t("revokeDeploymentToken")}
        onClick={revoke}
        loading={loading}
        disabled={!id || loading}
      >
        <Prohibit size={14} />
        {t("revoke")}
      </Button>
      {error && (
        <span className="max-w-48 text-xs leading-5 text-kumo-danger">
          {error}
        </span>
      )}
    </div>
  );
}
