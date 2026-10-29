import { useQuery } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Navigate, Route, Routes, useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { AppShell } from "./components/AppShell";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { GeoRoutingPage } from "./pages/GeoRoutingPage";
import { MessageCenterPage } from "./pages/MessageCenterPage";
import { MyProfilePage } from "./pages/MyProfilePage";
import { NotificationRoutingPage } from "./pages/NotificationRoutingPage";
import { OAuthActionPage } from "./pages/OAuthActionPage";
import { OverviewPage } from "./pages/OverviewPage";
import { ServerCommandsPage } from "./pages/ServerCommandsPage";
import { SystemSettingsPage } from "./pages/SystemSettingsPage";
import { WebClientSettingsPage } from "./pages/WebClientSettingsPage";
import { SelfSignedClientPage } from "./features/self-signed-client/SelfSignedClientPage";
import { ResourcePage } from "./resource/ResourcePage";
import { ALL_RESOURCES, resourcePath } from "./resource/registry";
import { apiGet } from "./lib/api";
import { currentUserQueryKey } from "./lib/auth";
import {
  authenticatedHome,
  canAccessConsolePath,
  hasAdminAccess,
} from "./lib/access";

interface CurrentUserAccess {
  route_names?: string[];
}

export default function AuthenticatedApp() {
  const { t } = useTranslation();
  const location = useLocation();
  const currentUser = useQuery({
    queryKey: currentUserQueryKey(),
    queryFn: () => apiGet<CurrentUserAccess>("/api/admin/user/current"),
  });

  if (currentUser.isPending) {
    return (
      <div
        className="flex min-h-full items-center justify-center bg-kumo-base p-6 text-sm text-kumo-subtle"
        role="status"
      >
        {t("loading")}
      </div>
    );
  }

  if (currentUser.isError || !currentUser.data) {
    return (
      <div className="flex min-h-full items-center justify-center bg-kumo-base p-6 text-kumo-default">
        <div className="max-w-md text-center">
          <h1 className="text-xl font-semibold">{t("adminLoadFailed")}</h1>
          <Button className="mt-5" onClick={() => void currentUser.refetch()}>
            {t("reloadPage")}
          </Button>
        </div>
      </div>
    );
  }

  const isAdmin = hasAdminAccess(currentUser.data.route_names);
  const home = authenticatedHome(isAdmin);
  if (!canAccessConsolePath(location.pathname, isAdmin)) {
    return <Navigate to={home} replace />;
  }

  return (
    <Routes>
      <Route element={<AppShell isAdmin={isAdmin} />}>
        <Route path="/" element={<Navigate to={home} replace />} />
        <Route path="/overview" element={<OverviewPage />} />
        <Route path="/diagnostics" element={<DiagnosticsPage />} />
        <Route path="/my" element={<MyProfilePage />} />
        <Route path="/messages" element={<MessageCenterPage />} />
        <Route
          path="/notification-routing"
          element={<NotificationRoutingPage />}
        />
        <Route path="/settings" element={<SystemSettingsPage />} />
        <Route path="/self-signed-client" element={<SelfSignedClientPage />} />
        <Route path="/serverCmd" element={<ServerCommandsPage />} />
        <Route path="/geo-routing" element={<GeoRoutingPage />} />
        <Route
          path="/webclient-settings"
          element={<WebClientSettingsPage />}
        />
        <Route path="/oauth/:code" element={<OAuthActionPage mode="confirm" />} />
        <Route
          path="/oauth/bind/:code"
          element={<OAuthActionPage mode="bind" />}
        />
        {ALL_RESOURCES.map((resource) => (
          <Route
            key={resource.name}
            path={resourcePath(resource)}
            element={<ResourcePage cfg={resource} />}
          />
        ))}
      </Route>
      <Route path="*" element={<Navigate to={home} replace />} />
    </Routes>
  );
}
