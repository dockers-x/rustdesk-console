const TOKEN_KEY = "api-token";
const OIDC_CODE_KEY = "oidc_code";
const OIDC_CODE_EXPIRY_KEY = "oidc_code_expiry";

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? "";
}

export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export function isLoggedIn(): boolean {
  return getToken().length > 0;
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
