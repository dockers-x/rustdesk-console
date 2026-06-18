import { apiGet } from "./api";

export type WebClientVersion = "v1" | "v2";

export const DEFAULT_WEB_CLIENT_VERSION: WebClientVersion = "v1";

interface WebClientCapabilities {
  v1: boolean;
  v2: boolean;
  preferred: WebClientVersion;
}

let capabilitiesPromise: Promise<WebClientCapabilities> | null = null;

function normalizeVersion(value: unknown): WebClientVersion {
  return value === "v2" ? "v2" : "v1";
}

export async function getWebClientCapabilities(): Promise<WebClientCapabilities> {
  if (!capabilitiesPromise) {
    capabilitiesPromise = apiGet<Partial<WebClientCapabilities>>(
      "/api/admin/config/webclient-capabilities",
    )
      .then((capabilities) => {
        const v2 = Boolean(capabilities.v2);
        return {
          v1: capabilities.v1 !== false,
          v2,
          preferred: v2 ? normalizeVersion(capabilities.preferred) : "v1",
        };
      })
      .catch(() => ({ v1: true, v2: false, preferred: "v1" }));
  }
  return capabilitiesPromise;
}

export async function getPreferredWebClientVersion() {
  return (await getWebClientCapabilities()).preferred;
}

export async function openWebClientPeer(
  peerId: string,
  version?: WebClientVersion,
) {
  const popup = window.open("about:blank", "_blank");
  const selectedVersion = version ?? (await getPreferredWebClientVersion());
  const url = webClientPeerUrl(peerId, selectedVersion);

  if (popup) {
    popup.opener = null;
    popup.location.href = url;
    return;
  }

  window.open(url, "_blank", "noopener,noreferrer");
}

export function webClientBasePath(
  version: WebClientVersion = DEFAULT_WEB_CLIENT_VERSION,
) {
  return version === "v2" ? "/webclient2/" : "/webclient/";
}

export function webClientPeerUrl(
  peerId: string,
  version: WebClientVersion = DEFAULT_WEB_CLIENT_VERSION,
) {
  const origin = window.location.origin.replace(/\/+$/, "");
  const safeId = encodeURIComponent(peerId.trim());
  const hash = version === "v2" ? `#/?id=${safeId}` : `#/${safeId}`;
  return `${origin}${webClientBasePath(version)}${hash}`;
}

export function webClientShareUrl(
  token: string,
  version: WebClientVersion = DEFAULT_WEB_CLIENT_VERSION,
) {
  const origin = window.location.origin.replace(/\/+$/, "");
  return `${origin}${webClientBasePath(version)}#/?share_token=${encodeURIComponent(
    token.trim(),
  )}`;
}

export function rustdeskNativeUri(peerId: string) {
  return `rustdesk://${encodeURIComponent(peerId.trim())}`;
}
