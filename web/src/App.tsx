import {
  Component,
  lazy,
  Suspense,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { Button } from "@cloudflare/kumo/components/button";
import {
  HashRouter,
  Navigate,
  Route,
  Routes,
  useLocation,
} from "react-router-dom";
import { useTranslation } from "react-i18next";
import { getAuthStateSnapshot, subscribeAuthState } from "./lib/auth";
import { AppTitleController } from "./lib/adminTitle";
import { ForceChangePasswordPage } from "./pages/ForceChangePasswordPage";
import { LoginPage } from "./pages/LoginPage";
import { RegisterPage } from "./pages/RegisterPage";

const AUTH_CHUNK_RELOAD_KEY = "rustdesk-console:auth-chunk-reload";

const AuthenticatedApp = lazy(async () => {
  try {
    const module = await import("./AuthenticatedApp");
    sessionStorage.removeItem(AUTH_CHUNK_RELOAD_KEY);
    return module;
  } catch (error) {
    if (sessionStorage.getItem(AUTH_CHUNK_RELOAD_KEY) !== "1") {
      sessionStorage.setItem(AUTH_CHUNK_RELOAD_KEY, "1");
      window.location.reload();
      return await new Promise<never>(() => undefined);
    }
    sessionStorage.removeItem(AUTH_CHUNK_RELOAD_KEY);
    throw error;
  }
});

function AuthenticatedRouteFallback() {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-full items-center justify-center bg-kumo-base p-6 text-kumo-default">
      <div className="max-w-md text-center">
        <h1 className="text-xl font-semibold">{t("adminLoadFailed")}</h1>
        <p className="mt-2 text-sm leading-6 text-kumo-subtle">
          {t("adminLoadFailedHint")}
        </p>
        <Button className="mt-5" onClick={() => window.location.reload()}>
          {t("reloadPage")}
        </Button>
      </div>
    </div>
  );
}

class AuthenticatedRouteBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    return this.state.failed ? (
      <AuthenticatedRouteFallback />
    ) : (
      this.props.children
    );
  }
}

function RequireAuth({
  children,
  allowPasswordChange = false,
}: {
  children: ReactNode;
  allowPasswordChange?: boolean;
}) {
  const location = useLocation();
  const authState = useSyncExternalStore(
    subscribeAuthState,
    getAuthStateSnapshot,
    () => "0:0",
  );
  const loggedIn = authState.startsWith("1:");
  const passwordChangeRequired = authState.endsWith(":1");
  if (!loggedIn) {
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
  if (passwordChangeRequired && !allowPasswordChange) {
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
              <AuthenticatedRouteBoundary>
                <Suspense fallback={<RouteLoading />}>
                  <AuthenticatedApp />
                </Suspense>
              </AuthenticatedRouteBoundary>
            </RequireAuth>
          }
        />
      </Routes>
    </HashRouter>
  );
}
