interface GuestLaunch {
  token: string;
  peer_id: string;
}

interface GuestConnection {
  peer_id: string;
  relay_ticket: string;
  peer_key: string;
  server_key: string;
}

const storageKey = "rustdesk-console.guest";

export const guestMode = location.pathname === "/webguest" || location.pathname.startsWith("/webguest/");

export const guestLaunch = (() => {
  try {
    const value = sessionStorage.getItem(storageKey);
    sessionStorage.removeItem(storageKey);
    if (guestMode && value) return JSON.parse(value) as GuestLaunch;
  } catch {
    // Regular WebClient connections do not need session storage.
  }
  return undefined;
})();

export async function connectGuest(id: string): Promise<GuestConnection> {
  if (!guestLaunch || id !== guestLaunch.peer_id) {
    throw new Error("The guest link is limited to its original device");
  }
  const response = await fetch("/api/web-guests/connect", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(guestLaunch),
    credentials: "omit",
    cache: "no-store",
  });
  if (!response.ok) throw new Error("The guest link is unavailable");
  const data = await response.json() as { code: number; data?: GuestConnection };
  if (data.code !== 0 || !data.data || data.data.peer_id !== id) {
    throw new Error("The guest link is unavailable or the device cannot be connected");
  }
  return data.data;
}
