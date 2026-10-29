import { afterEach, describe, expect, it, vi } from "vitest";
import { downloadGeneratedKeypair } from "./api";

vi.mock("../../lib/api", () => ({ apiGet: vi.fn() }));

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("generated signing key backup", () => {
  it("downloads the key pair once and returns only redacted settings for caching", async () => {
    vi.useFakeTimers();
    const click = vi.fn();
    const anchor = { href: "", download: "", click };
    vi.stubGlobal("document", { createElement: vi.fn(() => anchor) });
    const createObjectURL = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:key-backup");
    const revokeObjectURL = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const settings = {
      enabled: true,
      public_key: "test-public-key",
      key_configured: true,
      fingerprint: "test-fingerprint",
    };

    const result = downloadGeneratedKeypair({ ...settings, private_key: "test-private-key" });

    expect(result).toEqual(settings);
    expect(result).not.toHaveProperty("private_key");
    expect(click).toHaveBeenCalledTimes(1);
    expect(anchor.download).toBe("self-signed-client-keypair.json");
    const blob = createObjectURL.mock.calls[0][0] as Blob;
    expect(JSON.parse(await blob.text())).toEqual({
      algorithm: "Ed25519",
      public_key: settings.public_key,
      private_key: "test-private-key",
      fingerprint: settings.fingerprint,
    });
    await vi.runAllTimersAsync();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:key-backup");
  });
});
