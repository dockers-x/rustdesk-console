import axios, { type AxiosInstance } from "axios";
import { getToken, clearToken, setMustChangePassword } from "./auth";

/// Shared axios client. Sends the `api-token` header and unwraps the
/// `{ code, message, data }` envelope used by the admin API.
export const http: AxiosInstance = axios.create({ baseURL: "" });

http.interceptors.request.use((config) => {
  const token = getToken();
  if (token) {
    config.headers["api-token"] = token;
  }
  const lang = localStorage.getItem("lang");
  if (lang) config.headers["Accept-Language"] = lang;
  return config;
});

export class ApiError extends Error {
  code: number;
  constructor(code: number, message: string) {
    super(message);
    this.code = code;
  }
}

http.interceptors.response.use(
  (resp) => {
    const body = resp.data;
    // Envelope: { code, message, data }
    if (body && typeof body === "object" && "code" in body) {
      if (body.code === 0) return body.data;
      // 403 NeedLogin -> drop creds and let the app redirect to login.
      if (body.code === 403) clearToken();
      if (body.code === 112) {
        setMustChangePassword(true);
        if (window.location.hash !== "#/change-password") {
          window.location.hash = "#/change-password";
        }
      }
      throw new ApiError(body.code, body.message ?? "error");
    }
    return body;
  },
  (err) => Promise.reject(err),
);

/// GET helper that forwards query params.
export async function apiGet<T>(url: string, params?: Record<string, unknown>) {
  return (await http.get(url, { params })) as unknown as T;
}

export async function apiPost<T>(url: string, data?: unknown) {
  return (await http.post(url, data)) as unknown as T;
}

export async function apiPatch<T>(url: string, data?: unknown) {
  return (await http.patch(url, data)) as unknown as T;
}
