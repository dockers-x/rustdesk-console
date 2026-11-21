import { describe, expect, it } from "vitest";
import { summarizeLoginPlatform } from "./loginPlatform";

describe("summarizeLoginPlatform", () => {
  it("reports Electron ahead of the embedded Chrome version", () => {
    const raw = "Mozilla/5.0 (X11; Linux x86_64) Chrome/150.0 Electron/43.7.9 Safari/537.36";
    expect(summarizeLoginPlatform(raw)).toEqual({ summary: "Linux · Electron", raw });
  });

  it("preserves platform strings and identifies mobile operating systems", () => {
    expect(summarizeLoginPlatform("linux").summary).toBe("Linux");
    expect(summarizeLoginPlatform("Mozilla/5.0 (Linux; Android 16) Chrome/149.0").summary).toBe("Android · Chrome");
    expect(summarizeLoginPlatform("A custom client").summary).toBe("A custom client");
  });
});
