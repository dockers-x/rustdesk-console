const TOKEN_KEY = "api-token";
const AUTH_SESSION_KEY = "auth-session";
const MUST_CHANGE_PASSWORD_KEY = "must-change-password";
const OIDC_CODE_KEY = "oidc_code";
const OIDC_CODE_EXPIRY_KEY = "oidc_code_expiry";
const AUTH_STATE_EVENT = "rustdesk-console:auth-state";

function emitAuthStateChanged() {
  window.dispatchEvent(new Event(AUTH_STATE_EVENT));
}

function writeMustChangePassword(required: boolean) {
  if (required) {
    localStorage.setItem(MUST_CHANGE_PASSWORD_KEY, "1");
    return;
  }
  localStorage.removeItem(MUST_CHANGE_PASSWORD_KEY);
}

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? "";
}

function createAuthSessionId(): string {
  const values = new Uint32Array(4);
  crypto.getRandomValues(values);
  return Array.from(values, (value) => value.toString(16).padStart(8, "0")).join(
    "",
  );
}

export function getAuthSessionId(): string {
  if (!getToken()) return "";
  const existing = localStorage.getItem(AUTH_SESSION_KEY);
  if (existing) return existing;
  const sessionId = createAuthSessionId();
  localStorage.setItem(AUTH_SESSION_KEY, sessionId);
  return sessionId;
}

export function currentUserQueryKey(sessionId = getAuthSessionId()) {
  return ["current-user", sessionId] as const;
}

export function setToken(token: string, mustChangePassword = false) {
  localStorage.setItem(TOKEN_KEY, token);
  localStorage.setItem(AUTH_SESSION_KEY, createAuthSessionId());
  writeMustChangePassword(mustChangePassword);
  emitAuthStateChanged();
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(AUTH_SESSION_KEY);
  writeMustChangePassword(false);
  emitAuthStateChanged();
}

export function isLoggedIn(): boolean {
  return getToken().length > 0;
}

export function mustChangePassword(): boolean {
  return localStorage.getItem(MUST_CHANGE_PASSWORD_KEY) === "1";
}

export function setMustChangePassword(required: boolean) {
  writeMustChangePassword(required);
  emitAuthStateChanged();
}

export function getAuthStateSnapshot() {
  return `${isLoggedIn() ? "1" : "0"}:${getAuthSessionId()}:${mustChangePassword() ? "1" : "0"}`;
}

export function subscribeAuthState(onStoreChange: () => void) {
  const onStorage = (event: StorageEvent) => {
    if (
      event.key === null ||
      event.key === TOKEN_KEY ||
      event.key === AUTH_SESSION_KEY ||
      event.key === MUST_CHANGE_PASSWORD_KEY
    ) {
      onStoreChange();
    }
  };
  window.addEventListener(AUTH_STATE_EVENT, onStoreChange);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(AUTH_STATE_EVENT, onStoreChange);
    window.removeEventListener("storage", onStorage);
  };
}

export function setOidcCode(code: string) {
  sessionStorage.setItem(OIDC_CODE_KEY, code);
  sessionStorage.setItem(OIDC_CODE_EXPIRY_KEY, String(Date.now() + 5 * 60 * 1000));
}

export function getOidcCode(): string {
  const expiry = Number(sessionStorage.getItem(OIDC_CODE_EXPIRY_KEY) ?? 0);
  if (expiry && Date.now() > expiry) {
    clearOidcCode();
    return "";
  }
  return sessionStorage.getItem(OIDC_CODE_KEY) ?? "";
}

export function clearOidcCode() {
  sessionStorage.removeItem(OIDC_CODE_KEY);
  sessionStorage.removeItem(OIDC_CODE_EXPIRY_KEY);
}
