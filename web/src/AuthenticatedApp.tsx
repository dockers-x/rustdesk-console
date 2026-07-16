import { Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { MessageCenterPage } from "./pages/MessageCenterPage";
import { MyProfilePage } from "./pages/MyProfilePage";
import { NotificationRoutingPage } from "./pages/NotificationRoutingPage";
import { OAuthActionPage } from "./pages/OAuthActionPage";
import { OverviewPage } from "./pages/OverviewPage";
import { ServerCommandsPage } from "./pages/ServerCommandsPage";
import { SystemSettingsPage } from "./pages/SystemSettingsPage";
import { WebClientSettingsPage } from "./pages/WebClientSettingsPage";
import { ResourcePage } from "./resource/ResourcePage";
import { ALL_RESOURCES, resourcePath } from "./resource/registry";

const HOME = "/overview";

export default function AuthenticatedApp() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route path="/" element={<Navigate to={HOME} replace />} />
        <Route path="/overview" element={<OverviewPage />} />
        <Route path="/diagnostics" element={<DiagnosticsPage />} />
        <Route path="/my" element={<MyProfilePage />} />
        <Route path="/messages" element={<MessageCenterPage />} />
        <Route
          path="/notification-routing"
          element={<NotificationRoutingPage />}
        />
        <Route path="/settings" element={<SystemSettingsPage />} />
        <Route path="/serverCmd" element={<ServerCommandsPage />} />
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
      <Route path="*" element={<Navigate to={HOME} replace />} />
    </Routes>
  );
}
