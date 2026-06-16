/// Theme (light/dark) handling. kumo switches via `data-mode` on a parent
/// element (its tokens auto-adapt through CSS `light-dark()`), so we set
/// `data-mode` on <html> and persist the choice.

export type Mode = "light" | "dark";

const KEY = "mode";

export function getMode(): Mode {
  const saved = localStorage.getItem(KEY);
  if (saved === "light" || saved === "dark") return saved;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function applyMode(mode: Mode) {
  document.documentElement.setAttribute("data-mode", mode);
  document.documentElement.style.colorScheme = mode;
}

export function setMode(mode: Mode) {
  localStorage.setItem(KEY, mode);
  applyMode(mode);
}

/// Apply the persisted (or system) mode at startup. A `?mode=light|dark` query
/// param overrides and is persisted (handy for deep-linking / previewing).
export function initTheme() {
  const param = new URLSearchParams(window.location.search).get("mode");
  if (param === "light" || param === "dark") {
    setMode(param);
    return;
  }
  applyMode(getMode());
}
