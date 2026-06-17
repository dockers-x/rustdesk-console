import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { DownloadSimple } from "@phosphor-icons/react";
import { http } from "../lib/api";

export function RecordFileActions({
  id,
  filename,
}: {
  id: number;
  filename: string;
}) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const download = async () => {
    if (!id) return;
    setLoading(true);
    setError("");
    try {
      const blob = (await http.get(
        `/api/admin/record_file/download/${id}`,
        {
          responseType: "blob",
        },
      )) as unknown as Blob;
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = filename || `record-${id}`;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
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
        aria-label={`${t("download")} ${filename || id}`}
        onClick={download}
        loading={loading}
        disabled={!id || loading}
      >
        <DownloadSimple size={14} />
        {t("download")}
      </Button>
      {error && (
        <span className="max-w-48 text-xs leading-5 text-kumo-danger">
          {error}
        </span>
      )}
    </div>
  );
}
