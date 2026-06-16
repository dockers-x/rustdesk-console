import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { apiGet } from "./api";

interface PublicAdminConfig {
  title?: string;
  timezone?: string;
}

const normalizeTitle = (title?: string | null) => title?.trim() ?? "";

export function usePublicAdminConfig() {
  return useQuery({
    queryKey: ["public-admin-config"],
    queryFn: () => apiGet<PublicAdminConfig>("/api/admin/config/admin"),
    staleTime: 5 * 60 * 1000,
  });
}

export function useAppTitle() {
  const { t } = useTranslation();
  const config = usePublicAdminConfig();
  return normalizeTitle(config.data?.title) || t("appTitle");
}

export function AppTitleController() {
  const appTitle = useAppTitle();

  useEffect(() => {
    document.title = appTitle;
  }, [appTitle]);

  return null;
}
