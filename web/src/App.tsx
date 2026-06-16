import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { isLoggedIn, mustChangePassword } from "./lib/auth";
import { AppShell } from "./components/AppShell";
import { AppTitleController } from "./lib/adminTitle";
import { ForceChangePasswordPage } from "./pages/ForceChangePasswordPage";
import { LoginPage } from "./pages/LoginPage";
import { MyProfilePage } from "./pages/MyProfilePage";
import { OAuthActionPage } from "./pages/OAuthActionPage";
import { RegisterPage } from "./pages/RegisterPage";
import { ServerCommandsPage } from "./pages/ServerCommandsPage";
import { WebClientSettingsPage } from "./pages/WebClientSettingsPage";
import { ResourcePage } from "./resource/ResourcePage";
import { ALL_RESOURCES, RESOURCES, resourcePath } from "./resource/registry";

function RequireAuth({
  children,
  allowPasswordChange = false,
}: {
  children: React.ReactNode;
  allowPasswordChange?: boolean;
}) {
  if (!isLoggedIn()) return <Navigate to="/login" replace />;
  if (mustChangePassword() && !allowPasswordChange) {
    return <Navigate to="/change-password" replace />;
  }
  return <>{children}</>;
}

const HOME = `/${RESOURCES[0].name}`;

export default function App() {
  return (
    <HashRouter>
      <AppTitleController />
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route
          path="/change-password"
          element={
            <RequireAuth allowPasswordChange>
              <ForceChangePasswordPage />
            </RequireAuth>
          }
        />
        <Route
          element={
            <RequireAuth>
              <AppShell />
            </RequireAuth>
          }
        >
          <Route path="/" element={<Navigate to={HOME} replace />} />
          <Route path="/my" element={<MyProfilePage />} />
          <Route path="/serverCmd" element={<ServerCommandsPage />} />
          <Route path="/webclient-settings" element={<WebClientSettingsPage />} />
          <Route path="/oauth/:code" element={<OAuthActionPage mode="confirm" />} />
          <Route path="/oauth/bind/:code" element={<OAuthActionPage mode="bind" />} />
          {ALL_RESOURCES.map((r) => (
            <Route
              key={r.name}
              path={resourcePath(r)}
              element={<ResourcePage cfg={r} />}
            />
          ))}
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </HashRouter>
  );
}
