import { lazy, Suspense, type ReactNode } from "react";
import {
  HashRouter,
  Navigate,
  Route,
  Routes,
  useLocation,
} from "react-router-dom";
import { useTranslation } from "react-i18next";
import { isLoggedIn, mustChangePassword } from "./lib/auth";
import { AppTitleController } from "./lib/adminTitle";
import { ForceChangePasswordPage } from "./pages/ForceChangePasswordPage";
import { LoginPage } from "./pages/LoginPage";
import { RegisterPage } from "./pages/RegisterPage";

const AuthenticatedApp = lazy(() => import("./AuthenticatedApp"));

function RequireAuth({
  children,
  allowPasswordChange = false,
}: {
  children: ReactNode;
  allowPasswordChange?: boolean;
}) {
  const location = useLocation();
  if (!isLoggedIn()) {
    return (
      <Navigate
        to="/login"
        replace
        state={{
          from: `${location.pathname}${location.search}`,
        }}
      />
    );
  }
  if (mustChangePassword() && !allowPasswordChange) {
    return <Navigate to="/change-password" replace />;
  }
  return <>{children}</>;
}

function RouteLoading() {
  const { t } = useTranslation();
  return (
    <div
      className="flex min-h-full items-center justify-center bg-kumo-base p-6 text-sm text-kumo-subtle"
      role="status"
    >
      {t("loading")}
    </div>
  );
}

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
          path="/*"
          element={
            <RequireAuth>
              <Suspense fallback={<RouteLoading />}>
                <AuthenticatedApp />
              </Suspense>
            </RequireAuth>
          }
        />
      </Routes>
    </HashRouter>
  );
}
