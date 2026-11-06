import { describe, expect, it } from "vitest";
import {
  authenticatedHome,
  canAccessConsolePath,
  hasAdminAccess,
} from "./access";

describe("console access", () => {
  it("recognizes the server's admin route marker", () => {
    expect(hasAdminAccess(["*"])).toBe(true);
    expect(hasAdminAccess(["MyInfo", "MyPeer"])).toBe(false);
    expect(hasAdminAccess()).toBe(false);
  });

  it("uses a role-appropriate landing page", () => {
    expect(authenticatedHome(true)).toBe("/overview");
    expect(authenticatedHome(false)).toBe("/my");
  });

  it("keeps standard users in personal and messaging routes", () => {
    expect(canAccessConsolePath("/my", false)).toBe(true);
    expect(canAccessConsolePath("/my/peers", false)).toBe(true);
    expect(canAccessConsolePath("/messages", false)).toBe(true);
    expect(canAccessConsolePath("/developer/keys", false)).toBe(true);
    expect(canAccessConsolePath("/developer/sdks", false)).toBe(true);
    expect(canAccessConsolePath("/developer/openapi", false)).toBe(true);
    expect(canAccessConsolePath("/developer/admin", false)).toBe(false);
    expect(canAccessConsolePath("/messages?folder=sent", false)).toBe(true);
    expect(canAccessConsolePath("/oauth/bind/code", false)).toBe(true);

    expect(canAccessConsolePath("/overview", false)).toBe(false);
    expect(canAccessConsolePath("/users", false)).toBe(false);
    expect(canAccessConsolePath("/settings", false)).toBe(false);
    expect(canAccessConsolePath("/notification-routing", false)).toBe(false);
  });

  it("allows administrators to use every console route", () => {
    expect(canAccessConsolePath("/overview", true)).toBe(true);
    expect(canAccessConsolePath("/users", true)).toBe(true);
    expect(canAccessConsolePath("/settings", true)).toBe(true);
  });

  it("rejects paths that React Router could interpret as external", () => {
    expect(canAccessConsolePath("//example.com", true)).toBe(false);
    expect(canAccessConsolePath("/\\example.com", true)).toBe(false);
    expect(canAccessConsolePath("https://example.com", true)).toBe(false);
  });
});
