const ADMIN_ROUTE = "*";

export function hasAdminAccess(routeNames?: readonly string[]): boolean {
  return routeNames?.includes(ADMIN_ROUTE) ?? false;
}

export function authenticatedHome(isAdmin: boolean): string {
  return isAdmin ? "/overview" : "/my";
}

export function canAccessConsolePath(
  location: string,
  isAdmin: boolean,
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
