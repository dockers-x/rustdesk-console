import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { isLoggedIn } from "./lib/auth";
import { AppShell } from "./components/AppShell";
import { LoginPage } from "./pages/LoginPage";
import { MyProfilePage } from "./pages/MyProfilePage";
import { OAuthActionPage } from "./pages/OAuthActionPage";
import { RegisterPage } from "./pages/RegisterPage";
import { ServerCommandsPage } from "./pages/ServerCommandsPage";
import { ResourcePage } from "./resource/ResourcePage";
import { ALL_RESOURCES, RESOURCES, resourcePath } from "./resource/registry";

function RequireAuth({ children }: { children: React.ReactNode }) {
  if (!isLoggedIn()) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

const HOME = `/${RESOURCES[0].name}`;

export default function App() {
  return (
    <HashRouter>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
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
