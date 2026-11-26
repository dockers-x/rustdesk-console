import { useEffect, useMemo, useRef, useState } from "react";
import type { ComponentType } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Banner } from "@cloudflare/kumo/components/banner";
import { Button } from "@cloudflare/kumo/components/button";
import { cn } from "@cloudflare/kumo/utils";
import {
  BookBookmark,
  ChartBar,
  ClockCounterClockwise,
  Desktop,
  FileText,
  FlowArrow,
  Folders,
  GearSix,
  GlobeHemisphereWest,
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
  X,
} from "@phosphor-icons/react";
import { clearToken, getAuthSessionId } from "../lib/auth";
import { canAccessConsolePath, type RestrictedConsoleRole } from "../lib/access";
import { useAppBranding } from "../lib/adminTitle";
import { storagePath } from "../lib/storageProfiles";
import { getMode, setMode } from "../lib/theme";
import { selfSignedEnabled, useFeatures } from "../features/self-signed-client/api";
import {
  ADMIN_RESOURCES,
  MY_RESOURCES,
  resourcePath,
} from "../resource/registry";
import { LanguageSelect } from "./LanguageSelect";

type IconType = ComponentType<{ size?: number; weight?: "regular" | "fill" }>;

/// Sidebar icon per nav key (titleKey). Every configured route should have
/// a real icon so collapsed navigation remains recognizable.
const NAV_ICONS: Record<string, IconType> = {
  overview: ChartBar,
  myInfo: User,
  messageCenter: Bell,
  notificationRouting: FlowArrow,
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
  deviceAdmission: ShieldCheck,
  collections: Folders,
  shareRules: ShieldCheck,
  shareRecords: ShareNetwork,
  userTokens: Ticket,
  loginLogs: SignIn,
  activeConnections: PlugsConnected,
  auditConn: PlugsConnected,
  auditFile: FileText,
  consoleAudit: ShieldCheck,
  delegationTitle: UsersThree,
  recordFiles: VideoCamera,
  serverCommands: Terminal,
  geoRouting: GlobeHemisphereWest,
  systemSettings: GearSix,
  storageConfiguration: HardDrives,
  diagnostics: Pulse,
  webClientSettings: SlidersHorizontal,
  selfSignedClients: ShieldCheck,
  developerKeys: Key,
  developerSdks: Terminal,
  developerOpenapi: FileText,
};

interface NavItem {
  to: string;
  key: string;
  end?: boolean;
  indent?: boolean;
  feature?: "self_signed_client";
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
      myItem("myAddressBook"),
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
      { to: "/device-admission", key: "deviceAdmission" },
      { to: "/blocklist", key: "blocklistTitle" },
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
      { to: "/delegated-admin", key: "delegationTitle" },
      adminItem("users"),
      adminItem("groups"),
      adminItem("oauth"),
      adminItem("userTokens"),
    ],
  },
  {
    key: "auditRecords",
    items: [
      adminItem("consoleAudit"),
      adminItem("loginLogs"),
      adminItem("auditConn"),
      adminItem("auditFile"),
      adminItem("recordFiles"),
    ],
  },
  {
    key: "developerAccess",
    items: [
      { to: "/developer/keys", key: "developerKeys" },
      { to: "/developer/sdks", key: "developerSdks" },
      { to: "/developer/openapi", key: "developerOpenapi" },
    ],
  },
  {
    key: "systemOps",
    items: [
      { to: "/settings", key: "systemSettings" },
      { to: storagePath, key: "storageConfiguration" },
      { to: "/serverCmd", key: "serverCommands" },
      { to: "/geo-routing", key: "geoRouting" },
      { to: "/webclient-settings", key: "webClientSettings" },
      { to: "/self-signed-client", key: "selfSignedClients", feature: "self_signed_client" },
    ],
  },
];

const isItemActive = (pathname: string, item: NavItem) => {
  if (item.end) return pathname === item.to;
  return pathname === item.to || pathname.startsWith(`${item.to}/`);
};

const MOBILE_QUERY = "(max-width: 767px)";
const TOTP_REMINDER_SKIP_KEY = "admin-totp-reminder-skip-session";

export function AppShell({
  isAdmin,
  role,
  showTotpReminder,
}: {
  isAdmin: boolean;
  role?: RestrictedConsoleRole;
  showTotpReminder: boolean;
}) {
  const { t } = useTranslation();
  const features = useFeatures(isAdmin);
  const showSelfSignedClients = selfSignedEnabled(features.data);
  const { title: appTitle, logoUrl } = useAppBranding();
  const navigate = useNavigate();
  const location = useLocation();
  const authSession = getAuthSessionId();
  const [skippedTotpSession, setSkippedTotpSession] = useState(
    () => localStorage.getItem(TOTP_REMINDER_SKIP_KEY),
  );
  const skipTotpReminder = () => {
    localStorage.setItem(TOTP_REMINDER_SKIP_KEY, authSession);
    setSkippedTotpSession(authSession);
  };
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
  const workspaceRef = useRef<HTMLElement>(null);
  const [dark, setDark] = useState(() => getMode() === "dark");
  const visibleNavSections = useMemo(
    () =>
      NAV_SECTIONS.map((section) => ({
        ...section,
        items: section.items.filter((item) =>
          canAccessConsolePath(item.to, isAdmin, role) && (!item.feature || showSelfSignedClients),
        ),
      })).filter((section) => section.items.length > 0),
    [isAdmin, role, showSelfSignedClients],
  );
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
    if (!location.hash) workspaceRef.current?.scrollTo(0, 0);
  }, [location.pathname, location.hash]);

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

  const logout = () => {
    clearToken();
    navigate("/login", { replace: true });
  };

  const closeMobileNav = () => {
    if (isMobile) setMobileNavOpen(false);
  };

  const navCompact = !isMobile && desktopCollapsed;
  const navExpanded = isMobile ? mobileNavOpen : !desktopCollapsed;
  const activeSection = visibleNavSections.find((section) =>
    section.items.some((item) => isItemActive(location.pathname, item)),
  );
  const activeItem = activeSection?.items.find((item) =>
    isItemActive(location.pathname, item),
  );

  useEffect(() => {
    const activeSection = visibleNavSections.find((section) =>
      section.items.some((item) => isItemActive(location.pathname, item)),
    );
    if (!activeSection) return;
    setOpenSections((state) =>
      state[activeSection.key] ? state : { ...state, [activeSection.key]: true },
    );
  }, [location.pathname, visibleNavSections]);

  return (
    <div className="admin-shell relative flex h-full overflow-hidden bg-kumo-base text-kumo-default">
      <a
        href="#main-content"
        inert={isMobile && mobileNavOpen}
        onClick={(event) => {
          event.preventDefault();
          workspaceRef.current?.focus();
        }}
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-kumo-elevated focus:px-3 focus:py-2 focus:text-sm focus:shadow"
      >
        {t("skipToContent")}
      </a>
      {isMobile && (
        <button
          type="button"
          tabIndex={-1}
          inert={!mobileNavOpen}
          aria-hidden
          data-open={mobileNavOpen}
          className="admin-mobile-backdrop fixed inset-0 z-30 bg-black/45"
          aria-label={t("collapseSidebar")}
          onClick={() => setMobileNavOpen(false)}
        />
      )}
      <aside
        id="console-navigation"
        ref={mobileNavRef}
        onKeyDown={(event) => {
          if (!mobileNavOpen || event.key !== "Tab") return;
          const items = event.currentTarget.querySelectorAll<HTMLElement>("button:not([disabled]), a[href]");
          const first = items[0];
          const last = items[items.length - 1];
          if (event.shiftKey && event.target === first) {
            event.preventDefault();
            last?.focus();
          } else if (!event.shiftKey && event.target === last) {
            event.preventDefault();
            first?.focus();
          }
        }}
        className={cn(
          "admin-mobile-nav fixed inset-y-0 left-0 z-40 flex shrink-0 flex-col border-r border-kumo-line bg-kumo-elevated",
          "transition-transform duration-200 motion-reduce:transition-none",
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
              : 240,
        }}
        aria-hidden={isMobile && !mobileNavOpen}
        aria-label={isMobile ? t("mainNavigation") : undefined}
        aria-modal={isMobile ? true : undefined}
        inert={isMobile && !mobileNavOpen}
        role={isMobile ? "dialog" : undefined}
      >
        <div
          className={cn(
            "flex h-14 shrink-0 items-center gap-2.5 border-b border-kumo-line px-4 font-semibold",
            navCompact && "justify-center px-0",
          )}
        >
          <img src={logoUrl} alt="" className="size-8 shrink-0 object-contain" />
          {!navCompact && (
            <span className="truncate" title={appTitle}>
              {appTitle}
            </span>
          )}
          {isMobile && (
            <Button variant="ghost" className="ml-auto min-h-11 min-w-11" aria-label={t("collapseSidebar")} onClick={closeMobileNav}>
              <X size={18} aria-hidden />
            </Button>
          )}
        </div>
        <nav
          className="flex-1 overflow-y-auto px-2 py-2"
          aria-label={t("mainNavigation")}
        >
          {visibleNavSections.map((section, sectionIndex) => {
            const sectionActive = section.items.some((item) =>
              isItemActive(location.pathname, item),
            );
            return (
              <div
                key={section.key}
                className={cn(
                  "pb-2",
                  sectionIndex > 0 && "mt-1 border-t border-kumo-line/60 pt-2",
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
                      aria-label={t(item.key)}
                      onClick={closeMobileNav}
                      className={({ isActive }) =>
                        cn(
                          "admin-nav-link relative flex min-h-9 items-center gap-2.5 rounded-lg px-3 py-2 text-sm max-md:min-h-11",
                          navCompact && "justify-center",
                          item.indent && !navCompact && "pl-7",
                          isActive
                            ? "bg-kumo-tint font-semibold text-kumo-default"
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
        <header className="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-kumo-line bg-kumo-base px-3 sm:px-5">
          <div className="flex min-w-0 items-center gap-3">
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
            aria-controls="console-navigation"
            aria-label={
              navExpanded ? t("collapseSidebar") : t("expandSidebar")
            }
            title={navExpanded ? t("collapseSidebar") : t("expandSidebar")}
          >
            <List size={18} />
          </Button>
          {activeSection && activeItem && (
            <div className="flex min-w-0 items-center gap-2 text-sm">
              <span className="hidden shrink-0 text-kumo-subtle sm:inline">{t(activeSection.key)}</span>
              <span className="hidden text-kumo-subtle sm:inline" aria-hidden="true">/</span>
              <span className="truncate font-medium" title={t(activeItem.key)}>{t(activeItem.key)}</span>
            </div>
          )}
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <LanguageSelect />
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
              className="min-h-11 min-w-11 justify-center md:min-h-0 md:min-w-0"
              onClick={logout}
              title={t("logout")}
            >
              <SignOut size={18} />
              <span className="sr-only sm:not-sr-only">
                {t("logout")}
              </span>
            </Button>
          </div>
        </header>
        <main id="main-content" ref={workspaceRef} tabIndex={-1} className="admin-workspace min-h-0 flex-1 overflow-auto p-4 sm:p-6 lg:p-8">
          <div className="mx-auto w-full max-w-[96rem]">
            {showTotpReminder && skippedTotpSession !== authSession && (
              <Banner
                className="mb-4"
                variant="alert"
                icon={<ShieldCheck size={18} />}
                title={t("adminTotpReminder")}
                action={
                  <>
                    <Banner.Action onClick={() => navigate("/my#account-security")}>
                      {t("accountSecurity")}
                    </Banner.Action>
                    <Banner.Action variant="secondary" onClick={skipTotpReminder}>
                      {t("adminTotpSkipSession")}
                    </Banner.Action>
                  </>
                }
              />
            )}
            <div className="admin-page min-w-0">
              <Outlet context={isAdmin} />
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}
