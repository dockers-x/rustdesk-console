import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import defaultLogo from "../assets/console-logo-64.png";
import defaultLargeLogo from "../assets/console-logo-128.png";
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
  const customLogo = config.data?.logo_url;
  return {
    title: normalizeTitle(config.data?.title) || t("appTitle"),
    logoUrl: customLogo || defaultLogo,
    largeLogoUrl: customLogo || defaultLargeLogo,
    faviconUrl: customLogo || defaultFavicon,
  };
}

export function AppTitleController() {
  const { title, faviconUrl } = useAppBranding();

  useEffect(() => {
    document.title = title;
  }, [title]);

  useEffect(() => {
    const favicon = document.querySelector<HTMLLinkElement>("#console-favicon");
    if (favicon) favicon.href = faviconUrl;
  }, [faviconUrl]);

  return null;
}
