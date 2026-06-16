const TOKEN_KEY = "api-token";
const MUST_CHANGE_PASSWORD_KEY = "must-change-password";
const OIDC_CODE_KEY = "oidc_code";
const OIDC_CODE_EXPIRY_KEY = "oidc_code_expiry";

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? "";
}

export function setToken(token: string, mustChangePassword = false) {
  localStorage.setItem(TOKEN_KEY, token);
  setMustChangePassword(mustChangePassword);
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
  setMustChangePassword(false);
}

export function isLoggedIn(): boolean {
  return getToken().length > 0;
}

export function mustChangePassword(): boolean {
  return localStorage.getItem(MUST_CHANGE_PASSWORD_KEY) === "1";
}

export function setMustChangePassword(required: boolean) {
  if (required) {
    localStorage.setItem(MUST_CHANGE_PASSWORD_KEY, "1");
    return;
  }
  localStorage.removeItem(MUST_CHANGE_PASSWORD_KEY);
}

export function setOidcCode(code: string) {
  localStorage.setItem(OIDC_CODE_KEY, code);
  localStorage.setItem(OIDC_CODE_EXPIRY_KEY, String(Date.now() + 60 * 1000));
}

export function getOidcCode(): string {
  const expiry = Number(localStorage.getItem(OIDC_CODE_EXPIRY_KEY) ?? 0);
  if (expiry && Date.now() > expiry) {
    clearOidcCode();
    return "";
  }
  return localStorage.getItem(OIDC_CODE_KEY) ?? "";
}

export function clearOidcCode() {
  localStorage.removeItem(OIDC_CODE_KEY);
  localStorage.removeItem(OIDC_CODE_EXPIRY_KEY);
}
