import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { isLoggedIn } from "./lib/auth";
import { AppShell } from "./components/AppShell";
import { LoginPage } from "./pages/LoginPage";
import { ResourcePage } from "./resource/ResourcePage";
import { RESOURCES } from "./resource/registry";

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
        <Route
          element={
            <RequireAuth>
              <AppShell />
            </RequireAuth>
          }
        >
          <Route path="/" element={<Navigate to={HOME} replace />} />
          {RESOURCES.map((r) => (
            <Route
              key={r.name}
              path={`/${r.name}`}
              element={<ResourcePage cfg={r} />}
            />
          ))}
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </HashRouter>
  );
}
