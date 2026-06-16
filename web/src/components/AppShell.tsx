import { useEffect, useState } from "react";
import type { ComponentType } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { cn } from "@cloudflare/kumo/utils";
import {
  BookBookmark,
  ClockCounterClockwise,
  Desktop,
  FileText,
  Folders,
  HardDrives,
  Key,
  List,
  Monitor,
  Moon,
  Rows,
  SlidersHorizontal,
  Sun,
  SignOut,
  AddressBook,
  ShareNetwork,
  ShieldCheck,
  Tag,
  Terminal,
  Ticket,
  SignIn,
  PlugsConnected,
  User,
  Users,
  UsersThree,
} from "@phosphor-icons/react";
import { clearToken } from "../lib/auth";
import { useAppTitle } from "../lib/adminTitle";
import { getMode, setMode } from "../lib/theme";
import {
  ADMIN_RESOURCES,
  MY_RESOURCES,
  resourcePath,
} from "../resource/registry";
import i18n from "../i18n";

type IconType = ComponentType<{ size?: number; weight?: "regular" | "fill" }>;

/// Sidebar icon per nav key (titleKey). Every configured route should have
/// a real icon so collapsed navigation remains recognizable.
const NAV_ICONS: Record<string, IconType> = {
  myInfo: User,
  myPeers: Desktop,
  myAddressBook: AddressBook,
  myCollections: BookBookmark,
  myTags: Tag,
  myShareRules: ShieldCheck,
  myShareRecords: ShareNetwork,
  myLoginLogs: ClockCounterClockwise,
  users: Users,
  groups: UsersThree,
  deviceGroups: HardDrives,
  tags: Tag,
  devices: Monitor,
  oauth: Key,
  addressBook: AddressBook,
  collections: Folders,
  shareRules: ShieldCheck,
  shareRecords: ShareNetwork,
  userTokens: Ticket,
  loginLogs: SignIn,
  activeConnections: PlugsConnected,
  auditConn: PlugsConnected,
  auditFile: FileText,
  serverCommands: Terminal,
  webClientSettings: SlidersHorizontal,
};

interface NavItem {
  to: string;
  key: string;
  end?: boolean;
  indent?: boolean;
}

const NAV_SECTIONS: { key: string; items: NavItem[] }[] = [
  {
    key: "personal",
    items: [
      { to: "/my", key: "myInfo", end: true },
      ...MY_RESOURCES.map((r) => ({
        to: resourcePath(r),
        key: r.titleKey,
        indent: true,
      })),
    ],
  },
  {
    key: "management",
    items: ADMIN_RESOURCES.map((r) => ({ to: resourcePath(r), key: r.titleKey })),
  },
  {
    key: "operations",
    items: [
      { to: "/serverCmd", key: "serverCommands" },
      { to: "/webclient-settings", key: "webClientSettings" },
    ],
  },
];

const isItemActive = (pathname: string, item: NavItem) => {
  if (item.end) return pathname === item.to;
  return pathname === item.to || pathname.startsWith(`${item.to}/`);
};

export function AppShell() {
  const { t } = useTranslation();
  const appTitle = useAppTitle();
  const navigate = useNavigate();
  const location = useLocation();
  const [collapsed, setCollapsed] = useState(
    () => typeof window !== "undefined" && window.innerWidth < 768,
  );
  const [dark, setDark] = useState(() => getMode() === "dark");

  useEffect(() => {
    const media = window.matchMedia("(max-width: 767px)");
    if (media.matches) setCollapsed(true);

    const onChange = (event: MediaQueryListEvent) => {
      if (event.matches) setCollapsed(true);
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

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

  const closeMobileNav = () => {
    if (window.innerWidth < 768) setCollapsed(true);
  };

  return (
    <div className="relative flex h-full overflow-hidden bg-kumo-base text-kumo-default">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-kumo-elevated focus:px-3 focus:py-2 focus:text-sm focus:shadow"
      >
        {t("skipToContent")}
      </a>
      <aside
        className={cn(
          "flex shrink-0 flex-col border-r border-kumo-line bg-kumo-elevated transition-[width] duration-150",
          !collapsed && "max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-40",
        )}
        style={{ width: collapsed ? 64 : 220 }}
      >
        <div
          className={cn(
            "flex h-14 items-center gap-2 px-4 font-semibold",
            collapsed && "justify-center px-0",
          )}
        >
          <Monitor size={20} />
          {!collapsed && (
            <span className="truncate" title={appTitle}>
              {appTitle}
            </span>
          )}
        </div>
        <nav className="flex-1 overflow-y-auto px-2 py-2" aria-label="main">
          {NAV_SECTIONS.map((section, sectionIndex) => {
            const sectionActive = section.items.some((item) =>
              isItemActive(location.pathname, item),
            );
            return (
              <div
                key={section.key}
                className={cn(
                  "pb-2",
                  sectionIndex > 0 && "mt-2 border-t border-kumo-line pt-2",
                )}
              >
                {!collapsed && (
                  <div
                    className={cn(
                      "px-3 pb-1 pt-1 text-xs font-semibold",
                      sectionActive ? "text-kumo-default" : "text-kumo-subtle",
                    )}
                  >
                    {t(section.key)}
                  </div>
                )}
                {section.items.map((item) => (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    end={item.end}
                    title={t(item.key)}
                    onClick={closeMobileNav}
                    className={({ isActive }) =>
                      cn(
                        "group relative flex min-h-9 items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-[background-color,color,box-shadow,scale] duration-150 active:scale-[0.98]",
                        collapsed && "justify-center",
                        item.indent && !collapsed && "pl-7",
                        isActive
                          ? "bg-kumo-tint font-medium text-kumo-default shadow-xs ring-1 ring-kumo-line/70"
                          : "text-kumo-subtle hover:bg-kumo-tint/70 hover:text-kumo-default",
                      )
                    }
                  >
                    {({ isActive }) => {
                      const Icon = NAV_ICONS[item.key] ?? Rows;
                      return (
                        <>
                          {item.indent && !collapsed && (
                            <span
                              className={cn(
                                "absolute left-3 top-1/2 h-4 w-px -translate-y-1/2 rounded-full",
                                isActive ? "bg-kumo-brand" : "bg-kumo-line",
                              )}
                              aria-hidden="true"
                            />
                          )}
                          <Icon size={18} weight={isActive ? "fill" : "regular"} />
                          {!collapsed && (
                            <span className="truncate">{t(item.key)}</span>
                          )}
                        </>
                      );
                    }}
                  </NavLink>
                ))}
              </div>
            );
          })}
        </nav>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center justify-between border-b border-kumo-line px-4">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setCollapsed((c) => !c)}
            aria-expanded={!collapsed}
            aria-label={collapsed ? t("expandSidebar") : t("collapseSidebar")}
            title={collapsed ? t("expandSidebar") : t("collapseSidebar")}
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
        <main id="main-content" className="min-h-0 flex-1 overflow-auto p-4 sm:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
