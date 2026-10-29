import { useState } from "react";
import { useTranslation } from "react-i18next";
import { InlineMessage } from "../../components/InlineMessage";

const filenames = ["logo.png", "logo_light.png", "logo_dark.png", "icon.png"] as const;
const controlClass = "inline-flex min-h-11 items-center justify-center rounded-md border border-kumo-line px-3 text-sm font-medium hover:bg-kumo-tint/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand";

async function readPng(file: File): Promise<string> {
  if (file.size === 0 || file.size > 128 * 1024) throw new Error("invalid");
  const bytes = new Uint8Array(await file.arrayBuffer());
  const signature = [137, 80, 78, 71, 13, 10, 26, 10];
  if (!signature.every((value, index) => bytes[index] === value)) throw new Error("invalid");
  if (bytes.length < 24 || String.fromCharCode(...bytes.slice(12, 16)) !== "IHDR") throw new Error("invalid");
  const header = new DataView(bytes.buffer);
  const width = header.getUint32(16);
  const height = header.getUint32(20);
  if (width === 0 || height === 0 || width > 2048 || height > 2048) throw new Error("invalid");
  const url = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
  try {
    await new Promise<void>((resolve, reject) => {
      const image = new Image();
      image.onload = () => image.naturalWidth > 0 && image.naturalHeight > 0 && image.naturalWidth <= 2048 && image.naturalHeight <= 2048 ? resolve() : reject(new Error("invalid"));
      image.onerror = () => reject(new Error("invalid"));
      image.src = url;
    });
  } finally {
    URL.revokeObjectURL(url);
  }
  return btoa(Array.from(bytes, (value) => String.fromCharCode(value)).join(""));
}

export function BuildAssets({ assets, onChange }: {
  assets: Record<string, string>;
  onChange: (assets: Record<string, string>) => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function upload(filename: string, file: File) {
    setBusy(filename);
    setError(null);
    try {
      const content = await readPng(file);
      onChange({ ...assets, [filename]: content });
    } catch {
      setError(filename);
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="min-w-0 space-y-4">
      <div>
        <h2 className="text-base font-semibold">{t("selfSignedAssetsTitle")}</h2>
        <p className="mt-1 text-sm leading-6 text-kumo-subtle">{t("selfSignedAssetsHint")}</p>
        <p className="mt-2 text-sm leading-6 text-kumo-subtle">{t("selfSignedAssetsBuild")}</p>
      </div>
      <div className="grid min-w-0 gap-4 lg:grid-cols-2">
        {filenames.map((filename) => {
          const content = assets[filename];
          const source = content ? `data:image/png;base64,${content}` : undefined;
          const background = filename === "logo_dark.png" ? "bg-slate-950" : filename === "logo_light.png" ? "bg-white" : "bg-slate-200";
          const previewLabel = filename === "icon.png" ? "selfSignedAssetIcon" : filename === "logo_dark.png" ? "selfSignedAssetDark" : filename === "logo_light.png" ? "selfSignedAssetLight" : "selfSignedAssetNeutral";
          return (
            <div key={filename} className="min-w-0 rounded-lg border border-kumo-line bg-kumo-elevated p-4">
              <h3 className="break-all font-mono text-sm font-medium">{filename}</h3>
              <p className="mt-1 text-xs text-kumo-subtle">{t(previewLabel)}</p>
              <div className={`mt-3 flex h-28 min-w-0 items-center justify-center overflow-hidden rounded-md border border-kumo-line p-3 ${background}`}>
                {source ? <img src={source} alt={filename} className={filename === "icon.png" ? "h-16 w-16 object-contain" : "max-h-[60px] w-full max-w-[300px] object-contain"} /> : <span className={`text-sm ${filename === "logo_dark.png" ? "text-slate-300" : "text-slate-600"}`}>{t("selfSignedAssetEmpty")}</span>}
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <label className={`${controlClass} relative focus-within:ring-2 focus-within:ring-kumo-brand ${busy ? "cursor-wait opacity-50" : "cursor-pointer"}`}>
                  {t(busy === filename ? "selfSignedAssetBusy" : "selfSignedAssetUpload")}
                  <input type="file" accept="image/png,.png" aria-label={`${t("selfSignedAssetUpload")}: ${filename}`} disabled={busy !== null} className="absolute inset-0 w-full cursor-pointer opacity-0" onChange={(event) => {
                    const file = event.currentTarget.files?.[0];
                    event.currentTarget.value = "";
                    if (file) void upload(filename, file);
                  }} />
                </label>
                {source && <>
                  <a className={controlClass} href={source} download={filename}>{t("selfSignedAssetDownload")}</a>
                  <button type="button" className={controlClass} disabled={busy !== null} onClick={() => {
                    const next = { ...assets };
                    delete next[filename];
                    onChange(next);
                  }}>{t("selfSignedAssetRemove")}</button>
                </>}
              </div>
              {error === filename && <div className="mt-3"><InlineMessage tone="error">{t("selfSignedAssetInvalid")}</InlineMessage></div>}
            </div>
          );
        })}
      </div>
    </section>
  );
}
