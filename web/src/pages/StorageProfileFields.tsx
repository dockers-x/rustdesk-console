import { useTranslation } from "react-i18next";
import { Input } from "@cloudflare/kumo/components/input";
import { Radio } from "@cloudflare/kumo/components/radio";
import { Switch } from "@cloudflare/kumo/components/switch";
import { ConfigField } from "../components/ConfigField";
import type { StorageConfig, StorageType } from "../lib/storageProfiles";

export function StorageProfileFields({ config, onChange, locked }: {
  config: StorageConfig;
  onChange: (next: StorageConfig) => void;
  locked: boolean;
}) {
  const { t } = useTranslation();
  const setRoot = (field: "local_dir" | "temp_dir", value: string) => onChange({ ...config, [field]: value });
  const setS3 = <K extends keyof StorageConfig["s3"]>(field: K, value: StorageConfig["s3"][K]) =>
    onChange({ ...config, s3: { ...config.s3, [field]: value } });
  const setWebDav = <K extends keyof StorageConfig["webdav"]>(field: K, value: StorageConfig["webdav"][K]) =>
    onChange({ ...config, webdav: { ...config.webdav, [field]: value } });
  const remote = config.type !== "local";

  return (
    <div className="grid gap-4">
      <Radio.Group<StorageType>
        legend={t("recordStorageBackend")}
        appearance="segmented"
        orientation="horizontal"
        value={config.type}
        disabled={locked}
        onValueChange={(type) => onChange({ ...config, type })}
      >
        <Radio.Item value="local" label={t("recordStorageLocal")} className="max-sm:min-h-11" />
        <Radio.Item value="s3" label="S3" className="max-sm:min-h-11" />
        <Radio.Item value="webdav" label="WebDAV" className="max-sm:min-h-11" />
      </Radio.Group>
      <div className="grid gap-4">
        {config.type === "local" && (
          <ConfigField
            disabled={locked}
            label={t("recordStorageLocalDir")}
            value={config.local_dir}
            onChange={(value) => setRoot("local_dir", value)}
            placeholder="resources/record"
            helper={t("recordStorageLocalDirHint")}
          />
        )}
        {remote && (
          <ConfigField
            disabled={locked}
            label={t("recordStorageTempDir")}
            value={config.temp_dir}
            onChange={(value) => setRoot("temp_dir", value)}
            placeholder="resources/record/tmp"
            helper={t("recordStorageTempDirHint")}
          />
        )}
      </div>

      {config.type === "s3" && (
        <div className="grid gap-4">
          <div className="grid gap-4 lg:grid-cols-2">
            <ConfigField
              disabled={locked}
              label={t("s3Endpoint")}
              value={config.s3.endpoint}
              onChange={(value) => setS3("endpoint", value)}
              placeholder="https://s3.example.com"
            />
            <ConfigField
              disabled={locked}
              label={t("s3Region")}
              value={config.s3.region}
              onChange={(value) => setS3("region", value)}
              placeholder="auto"
            />
            <ConfigField
              disabled={locked}
              label={t("s3Bucket")}
              value={config.s3.bucket}
              onChange={(value) => setS3("bucket", value)}
              placeholder="rustdesk-recordings"
            />
            <ConfigField
              disabled={locked}
              label={t("storagePrefix")}
              value={config.s3.prefix}
              onChange={(value) => setS3("prefix", value)}
              placeholder="record/"
            />
            <SecretConfigField
              label={t("s3AccessKeyId")}
              value={config.s3.access_key_id}
              configured={config.s3.access_key_id_configured}

              onChange={(value) => setS3("access_key_id", value)}

            />
            <SecretConfigField
              label={t("s3SecretAccessKey")}
              value={config.s3.secret_access_key}
              configured={config.s3.secret_access_key_configured}

              onChange={(value) => setS3("secret_access_key", value)}

            />
          </div>
          <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-2">
            <Switch
              label={t("s3ForcePathStyle")}
              labelTooltip={t("s3ForcePathStyleHint")}
              controlFirst={false}
              disabled={locked}
              checked={config.s3.force_path_style}
              onCheckedChange={(checked: boolean) =>
                setS3("force_path_style", checked)
              }
            />
          </div>
        </div>
      )}

      {config.type === "webdav" && (
        <div className="grid gap-4 lg:grid-cols-2">
          <ConfigField
            disabled={locked}
            label={t("webDavUrl")}
            value={config.webdav.url}
            onChange={(value) => setWebDav("url", value)}
            placeholder="https://dav.example.com/remote.php/dav/files/user"
          />
          <ConfigField
            disabled={locked}
            label={t("storagePrefix")}
            value={config.webdav.prefix}
            onChange={(value) => setWebDav("prefix", value)}
            placeholder="record/"
          />
          <ConfigField
            disabled={locked}
            label={t("webDavUsername")}
            value={config.webdav.username}
            onChange={(value) => setWebDav("username", value)}
            placeholder="rustdesk"
          />
          <SecretConfigField
            label={t("webDavPassword")}
            value={config.webdav.password}
            configured={config.webdav.password_configured}
            clear={Boolean(config.webdav.clear_password)}
            onChange={(value) => setWebDav("password", value)}
            onClearChange={(value) => setWebDav("clear_password", value)}
          />
        </div>
      )}
    </div>
  );
}

function SecretConfigField({
  label,
  value,
  configured,
  clear,
  onChange,
  onClearChange,
}: {
  label: string;
  value: string;
  configured: boolean;
  clear?: boolean;
  onChange: (value: string) => void;
  onClearChange?: (value: boolean) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-2">
      <label className="block min-w-0">
        <span className="mb-1 block text-sm font-medium">{label}</span>
        <Input
          aria-label={label}
          type="password"
          value={value}
          disabled={clear}
          onChange={(e) => onChange(e.target.value)}
          placeholder={
            configured
              ? t("secretConfiguredPlaceholder")
              : t("secretEmptyPlaceholder")
          }
          spellCheck={false}
          className="w-full min-w-0"
        />
      </label>
      {configured && onClearChange && (
        <div className="rounded-lg border border-kumo-line bg-kumo-base px-3 py-2">
          <Switch
            label={t("clearConfiguredSecret")}
            controlFirst={false}
            checked={clear}
            onCheckedChange={(checked: boolean) => onClearChange(checked)}
          />
        </div>
      )}
    </div>
  );
}
