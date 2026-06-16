import { useState } from "react";
import type { ComponentType } from "react";
import { NavLink, Outlet, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import {
  List,
  Monitor,
  Moon,
  Sun,
  SignOut,
  User,
  Users,
  UsersThree,
  Stack,
  Tag,
  Key,
  AddressBook,
  Folders,
  ShieldCheck,
  ShareNetwork,
  Ticket,
  SignIn,
  PlugsConnected,
  FileText,
  Terminal,
  Dot,
} from "@phosphor-icons/react";
import { clearToken } from "../lib/auth";
import { getMode, setMode } from "../lib/theme";
import {
  ADMIN_RESOURCES,
  MY_RESOURCES,
  resourcePath,
} from "../resource/registry";
import i18n from "../i18n";

type IconType = ComponentType<{ size?: number; weight?: "regular" | "fill" }>;

/// Sidebar icon per nav key (titleKey). Falls back to a dot.
const NAV_ICONS: Record<string, IconType> = {
  myInfo: User,
  users: Users,
  groups: UsersThree,
  deviceGroups: Stack,
  tags: Tag,
  devices: Monitor,
  oauth: Key,
  addressBook: AddressBook,
  collections: Folders,
  shareRules: ShieldCheck,
  shareRecords: ShareNetwork,
  userTokens: Ticket,
  loginLogs: SignIn,
  auditConn: PlugsConnected,
  auditFile: FileText,
  serverCommands: Terminal,
};

interface NavItem {
  to: string;
  key: string;
  end?: boolean;
}

const NAV_SECTIONS: { key: string; items: NavItem[] }[] = [
  {
    key: "personal",
    items: [
      { to: "/my", key: "myInfo", end: true },
      ...MY_RESOURCES.map((r) => ({ to: resourcePath(r), key: r.titleKey })),
    ],
  },
  {
    key: "management",
    items: ADMIN_RESOURCES.map((r) => ({ to: resourcePath(r), key: r.titleKey })),
  },
  {
    key: "operations",
    items: [{ to: "/serverCmd", key: "serverCommands" }],
  },
];

export function AppShell() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [collapsed, setCollapsed] = useState(false);
  const [dark, setDark] = useState(() => getMode() === "dark");

  const toggleTheme = () => {
    const next = !dark;
    setDark(next);
    setMode(next ? "dark" : "light");
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
    <div className="flex h-full bg-kumo-base text-kumo-default">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-kumo-elevated focus:px-3 focus:py-2 focus:text-sm focus:shadow"
      >
        {t("skipToContent")}
      </a>
      <aside
        className="flex flex-col border-r border-kumo-line bg-kumo-elevated transition-all"
        style={{ width: collapsed ? 64 : 220 }}
      >
        <div className="flex h-14 items-center gap-2 px-4 font-semibold">
          <Monitor size={20} />
          {!collapsed && <span className="truncate">{t("appTitle")}</span>}
        </div>
        <nav className="flex-1 space-y-0.5 overflow-y-auto px-2 py-2">
          {NAV_SECTIONS.map((section) => (
            <div key={section.key} className="pb-2">
              {!collapsed && (
                <div className="px-3 pb-1 pt-2 text-[11px] font-medium uppercase text-kumo-subtle">
                  {t(section.key)}
                </div>
              )}
              {section.items.map((item) => (
                <NavLink
                  key={item.to}
                  to={item.to}
                  end={item.end}
                  title={t(item.key)}
                  className={({ isActive }) =>
                    [
                      "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors",
                      collapsed && "justify-center",
                      isActive
                        ? "bg-kumo-tint font-medium text-kumo-default"
                        : "text-kumo-subtle hover:bg-kumo-tint/60 hover:text-kumo-default",
                    ]
                      .filter(Boolean)
                      .join(" ")
                  }
                >
                  {(() => {
                    const Icon = NAV_ICONS[item.key] ?? Dot;
                    return <Icon size={18} />;
                  })()}
                  {!collapsed && <span className="truncate">{t(item.key)}</span>}
                </NavLink>
              ))}
            </div>
          ))}
        </nav>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center justify-between border-b border-kumo-line px-4">
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
        <main id="main-content" className="min-h-0 flex-1 overflow-auto p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
