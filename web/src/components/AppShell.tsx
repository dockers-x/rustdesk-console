import { useState } from "react";
import { NavLink, Outlet, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { List, Moon, Sun, SignOut } from "@phosphor-icons/react";
import { clearToken } from "../lib/auth";
import i18n from "../i18n";

const NAV = [
  { to: "/users", key: "users" },
  { to: "/devices", key: "devices" },
  { to: "/groups", key: "groups" },
  { to: "/tags", key: "tags" },
] as const;

function applyTheme(dark: boolean) {
  document.documentElement.style.colorScheme = dark ? "dark" : "light";
  document.documentElement.classList.toggle("dark", dark);
}

export function AppShell() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [collapsed, setCollapsed] = useState(false);
  const [dark, setDark] = useState(
    () => document.documentElement.classList.contains("dark"),
  );

  const toggleTheme = () => {
    const next = !dark;
    setDark(next);
    applyTheme(next);
  };

  const toggleLang = () => {
    const next = i18n.language === "zh-CN" ? "en" : "zh-CN";
    localStorage.setItem("lang", next);
    void i18n.changeLanguage(next);
  };

  const logout = () => {
    clearToken();
    navigate("/login", { replace: true });
  };

  return (
    <div className="flex h-full bg-kumo-base text-color-surface">
      <aside
        className="flex flex-col border-r border-color-border bg-kumo-elevated transition-all"
        style={{ width: collapsed ? 64 : 220 }}
      >
        <div className="flex h-14 items-center gap-2 px-4 font-semibold">
          <span className="text-lg">🖥️</span>
          {!collapsed && <span className="truncate">{t("appTitle")}</span>}
        </div>
        <nav className="flex-1 px-2 py-2">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                [
                  "block rounded-md px-3 py-2 text-sm transition-colors",
                  isActive
                    ? "bg-kumo-tint font-medium"
                    : "hover:bg-kumo-tint/60",
                ].join(" ")
              }
            >
              {collapsed ? t(item.key).charAt(0) : t(item.key)}
            </NavLink>
          ))}
        </nav>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center justify-between border-b border-color-border px-4">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setCollapsed((c) => !c)}
            aria-label="toggle sidebar"
          >
            <List size={18} />
          </Button>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={toggleLang}>
              {i18n.language === "zh-CN" ? "EN" : "中文"}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={toggleTheme}
              aria-label={t("theme")}
            >
              {dark ? <Sun size={18} /> : <Moon size={18} />}
            </Button>
            <Button variant="ghost" size="sm" onClick={logout}>
              <SignOut size={18} />
              <span className="ml-1">{t("logout")}</span>
            </Button>
          </div>
        </header>
        <main className="min-h-0 flex-1 overflow-auto p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
