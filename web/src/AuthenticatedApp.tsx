import { useQuery } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Navigate, Route, Routes, useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { AppShell } from "./components/AppShell";
import { ConsoleAuditRetention } from "./pages/ConsoleAuditRetention";
import { AddressBookPage } from "./pages/AddressBookPage";
import { DeviceAdmissionPage } from "./pages/DeviceAdmissionPage";
import { DelegatedAdminPage } from "./pages/DelegatedAdminPage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { GeoRoutingPage } from "./pages/GeoRoutingPage";
import { MessageCenterPage } from "./pages/MessageCenterPage";
import { EnrollTotpPage } from "./pages/EnrollTotpPage";
import { MyProfilePage } from "./pages/MyProfilePage";
import { NotificationRoutingPage } from "./pages/NotificationRoutingPage";
import { OAuthActionPage } from "./pages/OAuthActionPage";
import { OverviewPage } from "./pages/OverviewPage";
import { ServerCommandsPage } from "./pages/ServerCommandsPage";
import { StorageSettingsPage } from "./pages/StorageSettingsPage";
import { RecordingStorageSelection } from "./pages/RecordingStorageSelection";
import { storagePath } from "./lib/storageProfiles";
import { SystemSettingsPage } from "./pages/SystemSettingsPage";
import { WebClientSettingsPage } from "./pages/WebClientSettingsPage";
import { SelfSignedClientPage } from "./features/self-signed-client/SelfSignedClientPage";
import { DeveloperPage } from "./features/developer/DeveloperPage";
import { ResourcePage } from "./resource/ResourcePage";
import { PeerIdentitySummary } from "./components/PeerInfoDrawer";
import type { ResourceConfig } from "./resource/types";
import { ALL_RESOURCES, resourcePath } from "./resource/registry";
import { apiGet } from "./lib/api";
import { currentUserQueryKey } from "./lib/auth";
import {
  authenticatedHome,
  canAccessConsolePath,
  hasAdminAccess,
  restrictedConsoleRole,
  type RestrictedConsoleRole,
} from "./lib/access";

interface CurrentUserAccess {
  route_names?: string[];
  must_enroll_totp?: boolean;
  tfa_enabled?: boolean;
}

function resourceForRole(resource: ResourceConfig, role?: RestrictedConsoleRole): ResourceConfig {
  if (role === "audit") return { ...resource, canDelete: false };
  if (role === "devices" && resource.name === "peers") {
    return {
      ...resource,
      canDelete: false,
      columns: resource.columns
        .filter(({ key }) => key !== "__peer_actions")
        .map((column) => column.key === "id"
          ? { ...column, render: (row) => <PeerIdentitySummary row={row} /> }
          : column),
      fields: resource.fields.filter(({ name }) => name === "alias"),
    };
  }
  return resource;
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

  if (currentUser.data.must_enroll_totp) return <EnrollTotpPage />;

  const isAdmin = hasAdminAccess(currentUser.data.route_names);
  const role = restrictedConsoleRole(currentUser.data.route_names);
  const home = authenticatedHome(isAdmin, role);
  if (!canAccessConsolePath(location.pathname, isAdmin, role)) {
    return <Navigate to={home} replace />;
  }

  return (
    <Routes>
      <Route element={
        <AppShell
          isAdmin={isAdmin}
          role={role}
          showTotpReminder={isAdmin && currentUser.data.tfa_enabled === false}
        />
      }>
        <Route path="/" element={<Navigate to={home} replace />} />
        <Route path="/overview" element={<OverviewPage />} />
        <Route path="/device-admission" element={<DeviceAdmissionPage />} />
        <Route path="/delegated-admin" element={<DelegatedAdminPage />} />
        <Route path="/diagnostics" element={<DiagnosticsPage />} />
        <Route path="/my" element={<MyProfilePage />} />
        <Route path="/my/address_book" element={<AddressBookPage />} />
        <Route path="/messages" element={<MessageCenterPage />} />
        <Route
          path="/notification-routing"
          element={<NotificationRoutingPage />}
        />
        <Route path="/settings" element={<SystemSettingsPage />} />
        <Route path={storagePath} element={<StorageSettingsPage />} />
        <Route path="/self-signed-client" element={<SelfSignedClientPage />} />
        <Route path="/developer/keys" element={<DeveloperPage section="keys" isAdmin={isAdmin} />} />
        <Route path="/developer/sdks" element={<DeveloperPage section="sdks" isAdmin={isAdmin} />} />
        <Route path="/developer/openapi" element={<DeveloperPage section="openapi" isAdmin={isAdmin} />} />
        <Route path="/serverCmd" element={<ServerCommandsPage />} />
        <Route path="/geo-routing" element={<GeoRoutingPage />} />
        <Route
          path="/webclient-settings"
          element={<WebClientSettingsPage />}
        />
        <Route path="/oauth/:code" element={<OAuthActionPage />} />
        <Route
          path="/oauth/bind/:code"
          element={<Navigate to="/my#login-methods" replace />}
        />
        {ALL_RESOURCES.filter((resource) => resource.name !== "my_address_book").map((resource) => (
          <Route
            key={resource.name}
            path={resourcePath(resource)}
            element={
              <ResourcePage
                key={["peers", "address_book", "strategy_assignments"].includes(resource.name)
                  ? `${resource.name}${location.search}`
                  : resource.name}
                cfg={resourceForRole(resource, role)}
                accessScope={isAdmin ? "admin" : role ?? "member"}
                beforeTable={isAdmin && (
                  resource.name === "console-audit" ? <ConsoleAuditRetention /> :
                  resource.name === "record_file" ? <RecordingStorageSelection /> : undefined
                )}
              />
            }
          />
        ))}
      </Route>
      <Route path="*" element={<Navigate to={home} replace />} />
    </Routes>
  );
}
