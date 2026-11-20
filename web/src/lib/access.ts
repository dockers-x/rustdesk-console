const ADMIN_ROUTE = "*";

export function hasAdminAccess(routeNames?: readonly string[]): boolean {
  return routeNames?.includes(ADMIN_ROUTE) ?? false;
}

export type RestrictedConsoleRole = "audit" | "devices";

export function restrictedConsoleRole(routeNames?: readonly string[]): RestrictedConsoleRole | undefined {
  if (routeNames?.includes("AuditReader")) return "audit";
  if (routeNames?.includes("DeviceGroupOperator")) return "devices";
}

export function authenticatedHome(isAdmin: boolean, role?: RestrictedConsoleRole): string {
  if (isAdmin) return "/overview";
  if (role === "audit") return "/console-audit";
  if (role === "devices") return "/peers";
  return "/my";
}

export function canAccessConsolePath(
  location: string,
  isAdmin: boolean,
  role?: RestrictedConsoleRole,
): boolean {
  if (
    !location.startsWith("/") ||
    location.startsWith("//") ||
    location.includes("\\")
  ) {
    return false;
  }
  const pathname = location.split(/[?#]/, 1)[0];
  if (isAdmin) return true;
  if (role === "audit") {
    return ["/", "/console-audit", "/login_log", "/audit_conn", "/audit_file"].includes(pathname);
  }
  if (role === "devices") return pathname === "/" || pathname === "/peers";
  return (
    pathname === "/" ||
    pathname === "/my" ||
    pathname.startsWith("/my/") ||
    pathname === "/messages" ||
    pathname === "/developer/keys" ||
    pathname === "/developer/sdks" ||
    pathname === "/developer/openapi" ||
    pathname.startsWith("/oauth/")
  );
}
