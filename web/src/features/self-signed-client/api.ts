import { useQuery } from "@tanstack/react-query";
import { apiGet } from "../../lib/api";

export const featuresQueryKey = ["console-features"] as const;
export const selfSignedSettingsKey = ["self-signed-client-settings"] as const;
export const selfSignedProfileKey = ["self-signed-client-profile"] as const;
export const selfSignedBase = "/api/admin/features/self-signed-client";

export interface Features {
  self_signed_client: { enabled: boolean };
}

export interface SigningSettings {
  enabled: boolean;
  public_key: string;
  key_configured: boolean;
  fingerprint: string;
}

export interface ClientProfile {
  app_name: string;
  branding: Record<string, string>;
  default_settings: Record<string, string>;
  override_settings: Record<string, string>;
}

export interface ClientCatalog {
  branding: string[];
  config: string[];
  local: string[];
  display: string[];
  builtin: string[];
  expert_note: string;
}

export interface SignedConfig {
  filename: string;
  content: string;
  public_key: string;
  fingerprint: string;
  source_hint: string;
}

export const emptyProfile: ClientProfile = {
  app_name: "",
  branding: {},
  default_settings: {},
  override_settings: {},
};

export function selfSignedEnabled(features?: Features): boolean {
  return features?.self_signed_client?.enabled === true;
}

export function useFeatures(enabled = true) {
  return useQuery({
    queryKey: featuresQueryKey,
    queryFn: () => apiGet<Features>("/api/admin/features"),
    enabled,
    staleTime: 0,
    refetchOnMount: "always",
  });
}

export function downloadFile(filename: string, content: BlobPart, type = "text/plain;charset=utf-8") {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function featureError(error: unknown): string {
  const response = (error as { response?: { data?: { error?: unknown } } })?.response;
  if (typeof response?.data?.error === "string") return response.data.error;
  return error instanceof Error ? error.message : String(error);
}
