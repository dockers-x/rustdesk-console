import { useEffect, useRef, useState } from "react";
import type { ComponentType } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { cn } from "@cloudflare/kumo/utils";
import {
  BookBookmark,
  ChartBar,
  ClockCounterClockwise,
  Desktop,
  FileText,
  Folders,
  GearSix,
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
  Bell,
  CaretDown,
  ShareNetwork,
  ShieldCheck,
  Tag,
  Terminal,
  Ticket,
  SignIn,
  PlugsConnected,
  Pulse,
  User,
  Users,
  UsersThree,
  VideoCamera,
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
  overview: ChartBar,
  myInfo: User,
  messageCenter: Bell,
  notificationRouting: Bell,
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
  deploymentTokens: Ticket,
  strategies: SlidersHorizontal,
  strategyAssignments: Rows,
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
  recordFiles: VideoCamera,
  serverCommands: Terminal,
  systemSettings: GearSix,
  diagnostics: Pulse,
  webClientSettings: SlidersHorizontal,
};

interface NavItem {
  to: string;
  key: string;
  end?: boolean;
  indent?: boolean;
}

const adminItem = (key: string): NavItem => {
  const resource = ADMIN_RESOURCES.find((item) => item.titleKey === key);
  return { to: resource ? resourcePath(resource) : `/${key}`, key };
};

const myItem = (key: string): NavItem => {
  const resource = MY_RESOURCES.find((item) => item.titleKey === key);
  return { to: resource ? resourcePath(resource) : `/${key}`, key };
};

const NAV_SECTIONS: { key: string; items: NavItem[] }[] = [
  {
    key: "workspace",
    items: [
      { to: "/overview", key: "overview" },
      { to: "/diagnostics", key: "diagnostics" },
    ],
  },
  {
    key: "mySpace",
    items: [
      { to: "/my", key: "myInfo", end: true },
      myItem("myPeers"),
      myItem("myCollections"),
      myItem("myAddressBook"),
      myItem("myTags"),
      myItem("myShareRules"),
      myItem("myShareRecords"),
      myItem("myLoginLogs"),
    ],
  },
  {
    key: "messageNotifications",
    items: [
      { to: "/messages", key: "messageCenter" },
      { to: "/notification-routing", key: "notificationRouting" },
    ],
  },
  {
    key: "deviceAccess",
    items: [
      adminItem("devices"),
      adminItem("deviceGroups"),
      adminItem("addressBook"),
      adminItem("collections"),
      adminItem("shareRules"),
      adminItem("shareRecords"),
      adminItem("activeConnections"),
    ],
  },
  {
    key: "policyDeployment",
    items: [
      adminItem("deploymentTokens"),
      adminItem("strategies"),
      adminItem("strategyAssignments"),
      adminItem("tags"),
    ],
  },
  {
    key: "orgAccounts",
    items: [
      adminItem("users"),
      adminItem("groups"),
      adminItem("oauth"),
      adminItem("userTokens"),
      adminItem("loginLogs"),
    ],
  },
  {
    key: "auditRecords",
    items: [
      adminItem("auditConn"),
      adminItem("auditFile"),
      adminItem("recordFiles"),
    ],
  },
  {
    key: "systemOps",
    items: [
      { to: "/settings", key: "systemSettings" },
      { to: "/serverCmd", key: "serverCommands" },
      { to: "/webclient-settings", key: "webClientSettings" },
    ],
  },
];

const isItemActive = (pathname: string, item: NavItem) => {
  if (item.end) return pathname === item.to;
  return pathname === item.to || pathname.startsWith(`${item.to}/`);
};

const MOBILE_QUERY = "(max-width: 767px)";

export function AppShell() {
  const { t } = useTranslation();
  const appTitle = useAppTitle();
  const navigate = useNavigate();
  const location = useLocation();
  const [desktopCollapsed, setDesktopCollapsed] = useState(false);
  const [isMobile, setIsMobile] = useState(
    () =>
      typeof window !== "undefined" &&
      window.matchMedia(MOBILE_QUERY).matches,
  );
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const mobileNavRef = useRef<HTMLElement>(null);
  const mobileNavToggleRef = useRef<HTMLButtonElement>(null);
  const mobileNavWasOpen = useRef(false);
  const [dark, setDark] = useState(() => getMode() === "dark");
  const [openSections, setOpenSections] = useState<Record<string, boolean>>(
    () =>
      Object.fromEntries(
        NAV_SECTIONS.map((section) => [
          section.key,
          section.key === "workspace" ||
            section.items.some((item) => isItemActive(location.pathname, item)),
        ]),
      ),
  );

  useEffect(() => {
    const media = window.matchMedia(MOBILE_QUERY);
    const syncViewport = (matches: boolean) => {
      setIsMobile(matches);
      setMobileNavOpen(false);
    };
    syncViewport(media.matches);
    const onChange = (event: MediaQueryListEvent) => {
      syncViewport(event.matches);
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    if (!mobileNavOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMobileNavOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [mobileNavOpen]);

  useEffect(() => {
    if (isMobile) setMobileNavOpen(false);
  }, [isMobile, location.pathname, location.search]);

  useEffect(() => {
    if (!isMobile) {
      mobileNavWasOpen.current = false;
      return;
    }
    if (mobileNavOpen) {
      mobileNavWasOpen.current = true;
      const frame = window.requestAnimationFrame(() => {
        mobileNavRef.current
          ?.querySelector<HTMLElement>("button:not([disabled]), a[href]")
          ?.focus();
      });
      return () => window.cancelAnimationFrame(frame);
    }
    if (!mobileNavWasOpen.current) return;
    mobileNavWasOpen.current = false;
    const frame = window.requestAnimationFrame(() => {
      mobileNavToggleRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [isMobile, mobileNavOpen]);

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
    if (isMobile) setMobileNavOpen(false);
  };

  const navCompact = !isMobile && desktopCollapsed;
  const navExpanded = isMobile ? mobileNavOpen : !desktopCollapsed;

  useEffect(() => {
    const activeSection = NAV_SECTIONS.find((section) =>
      section.items.some((item) => isItemActive(location.pathname, item)),
    );
    if (!activeSection) return;
    setOpenSections((state) =>
      state[activeSection.key] ? state : { ...state, [activeSection.key]: true },
    );
  }, [location.pathname]);

  return (
    <div className="relative flex h-full overflow-hidden bg-kumo-base text-kumo-default">
      <a
        href="#main-content"
        inert={isMobile && mobileNavOpen}
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-kumo-elevated focus:px-3 focus:py-2 focus:text-sm focus:shadow"
      >
        {t("skipToContent")}
      </a>
      {isMobile && mobileNavOpen && (
        <button
          type="button"
          tabIndex={-1}
          className="admin-mobile-backdrop fixed inset-0 z-30 bg-black/50"
          aria-label={t("collapseSidebar")}
          onClick={() => setMobileNavOpen(false)}
        />
      )}
      <aside
        ref={mobileNavRef}
        className={cn(
          "admin-mobile-nav fixed inset-y-0 left-0 z-40 flex shrink-0 flex-col border-r border-kumo-line bg-kumo-elevated",
          "transition-transform duration-[180ms] motion-reduce:transition-none",
          "[transition-timing-function:var(--ease-out)]",
          isMobile && !mobileNavOpen && "pointer-events-none -translate-x-full",
          isMobile && mobileNavOpen && "translate-x-0",
          !isMobile && "relative z-auto translate-x-0 transition-none",
        )}
        style={{
          width: isMobile
            ? "min(280px, calc(100vw - 48px))"
            : navCompact
              ? 64
              : 220,
        }}
        aria-hidden={isMobile && !mobileNavOpen}
        aria-label={isMobile ? t("mainNavigation") : undefined}
        aria-modal={isMobile ? true : undefined}
        inert={isMobile && !mobileNavOpen}
        role={isMobile ? "dialog" : undefined}
      >
        <div
          className={cn(
            "flex h-14 items-center gap-2 px-4 font-semibold",
            navCompact && "justify-center px-0",
          )}
        >
          <Monitor size={20} />
          {!navCompact && (
            <span className="truncate" title={appTitle}>
              {appTitle}
            </span>
          )}
        </div>
        <nav
          className="flex-1 overflow-y-auto px-2 py-2"
          aria-label={t("mainNavigation")}
        >
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
                {!navCompact && (
                  <button
                    type="button"
                    className={cn(
                      "flex min-h-8 w-full items-center justify-between rounded-md px-3 pb-1 pt-1 text-left text-xs font-semibold transition-colors hover:bg-kumo-tint/70 max-md:min-h-11",
                      sectionActive ? "text-kumo-default" : "text-kumo-subtle",
                    )}
                    aria-expanded={openSections[section.key] ?? true}
                    onClick={() =>
                      setOpenSections((state) => ({
                        ...state,
                        [section.key]: !(state[section.key] ?? true),
                      }))
                    }
                  >
                    <span>{t(section.key)}</span>
                    <CaretDown
                      size={14}
                      className={cn(
                        "transition-transform",
                        !(openSections[section.key] ?? true) && "-rotate-90",
                      )}
                    />
                  </button>
                )}
                {(navCompact || (openSections[section.key] ?? true)) &&
                  section.items.map((item) => (
                    <NavLink
                      key={item.to}
                      to={item.to}
                      end={item.end}
                      title={t(item.key)}
                      onClick={closeMobileNav}
                      className={({ isActive }) =>
                        cn(
                          "group relative flex min-h-9 items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-[background-color,color,box-shadow,scale] duration-150 active:scale-[0.98] focus-visible:scale-100 max-md:min-h-11",
                          navCompact && "justify-center",
                          item.indent && !navCompact && "pl-7",
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
                            {item.indent && !navCompact && (
                              <span
                                className={cn(
                                  "absolute left-3 top-1/2 h-4 w-px -translate-y-1/2 rounded-full",
                                  isActive ? "bg-kumo-brand" : "bg-kumo-line",
                                )}
                                aria-hidden="true"
                              />
                            )}
                            <Icon size={18} weight={isActive ? "fill" : "regular"} />
                            {!navCompact && (
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

      <div
        className="flex min-w-0 flex-1 flex-col"
        inert={isMobile && mobileNavOpen}
      >
        <header className="flex h-14 items-center justify-between border-b border-kumo-line px-4">
          <Button
            ref={mobileNavToggleRef}
            variant="ghost"
            size="sm"
            className="min-h-11 min-w-11 justify-center md:min-h-0 md:min-w-0"
            onClick={() => {
              if (isMobile) setMobileNavOpen((open) => !open);
              else setDesktopCollapsed((collapsed) => !collapsed);
            }}
            aria-expanded={navExpanded}
            aria-label={
              navExpanded ? t("collapseSidebar") : t("expandSidebar")
            }
            title={navExpanded ? t("collapseSidebar") : t("expandSidebar")}
          >
            <List size={18} />
          </Button>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              className="min-h-11 md:min-h-0"
              onClick={toggleLang}
            >
              {i18n.language === "zh-CN" ? "EN" : "中文"}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="min-h-11 min-w-11 justify-center md:min-h-0 md:min-w-0"
              onClick={toggleTheme}
              aria-label={t("theme")}
            >
              {dark ? <Sun size={18} /> : <Moon size={18} />}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="min-h-11 md:min-h-0"
              onClick={logout}
            >
              <SignOut size={18} />
              <span className="ml-1 max-[359px]:sr-only max-[359px]:ml-0">
                {t("logout")}
              </span>
            </Button>
          </div>
        </header>
        <main id="main-content" className="min-h-0 flex-1 overflow-auto p-4 sm:p-6">
          <div key={location.pathname} className="admin-page-enter min-h-full">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}
