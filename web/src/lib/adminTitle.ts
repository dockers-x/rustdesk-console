import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { apiGet } from "./api";

interface PublicAdminConfig {
  title?: string;
  timezone?: string;
  logo_url?: string | null;
}

const normalizeTitle = (title?: string | null) => title?.trim() ?? "";
const defaultFavicon = "/api/admin/config/favicon";

export function usePublicAdminConfig() {
  return useQuery({
    queryKey: ["public-admin-config"],
    queryFn: () => apiGet<PublicAdminConfig>("/api/admin/config/admin"),
    staleTime: 5 * 60 * 1000,
  });
}

export function useAppBranding() {
  const { t } = useTranslation();
  const config = usePublicAdminConfig();
  return {
    title: normalizeTitle(config.data?.title) || t("appTitle"),
    logoUrl: config.data?.logo_url,
  };
}

export function AppTitleController() {
  const { title, logoUrl } = useAppBranding();

  useEffect(() => {
    document.title = title;
  }, [title]);

  useEffect(() => {
    const favicon = document.querySelector<HTMLLinkElement>("#console-favicon");
    if (favicon) favicon.href = logoUrl || defaultFavicon;
  }, [logoUrl]);

  return null;
}
