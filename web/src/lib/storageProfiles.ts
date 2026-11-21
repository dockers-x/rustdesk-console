import { useQuery } from "@tanstack/react-query";
import { apiGet } from "./api";

export type StorageType = "local" | "s3" | "webdav";
export interface StorageConfig {
  type: StorageType;
  local_dir: string;
  temp_dir: string;
  s3: {
    endpoint: string;
    region: string;
    bucket: string;
    prefix: string;
    access_key_id: string;
    secret_access_key: string;
    access_key_id_configured: boolean;
    secret_access_key_configured: boolean;
    force_path_style: boolean;
    clear_access_key_id?: boolean;
    clear_secret_access_key?: boolean;
  };
  webdav: {
    url: string;
    username: string;
    password: string;
    password_configured: boolean;
    prefix: string;
    clear_password?: boolean;
  };
}

export const emptyStorageConfig: StorageConfig = {
  type: "local",
  local_dir: "",
  temp_dir: "",
  s3: {
    endpoint: "",
    region: "auto",
    bucket: "",
    prefix: "record/",
    access_key_id: "",
    secret_access_key: "",
    access_key_id_configured: false,
    secret_access_key_configured: false,
    force_path_style: false,
    clear_access_key_id: false,
    clear_secret_access_key: false,
  },
  webdav: {
    url: "",
    username: "",
    password: "",
    password_configured: false,
    prefix: "record/",
    clear_password: false,
  },
};

export interface StorageProfile {
  id: string;
  name: string;
  config: StorageConfig;
  allow_http: boolean;
  archived?: boolean;
}

export interface BackupSelection {
  interval_hours: number;
  profile_ids: string[];
  age_recipient: string;
}

export interface StorageProfiles {
  entries: StorageProfile[];
  recording: string | null;
  backup: BackupSelection | null;
}

export const storagePath = "/storage";
export const storageApi = "/api/admin/storage";
export const profilesKey = [storageApi, "profiles"];
export const backupStatusKey = ["/api/admin/backups"];

export function useStorageProfiles() {
  return useQuery({
    queryKey: profilesKey,
    queryFn: () => apiGet<StorageProfiles>(`${storageApi}/profiles`),
  });
}
