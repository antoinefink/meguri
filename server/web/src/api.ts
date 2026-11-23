// SPDX-License-Identifier: MIT
export type Identity = { id: string; username: string; role: "admin" | "member" };
export type Me = Identity & { server_url: string };
export type AccountStorage = { collection_bytes: number; media_bytes: number; total_bytes: number };
export type Account = Identity & { enabled: boolean; provisioning_state: "provisioning" | "ready"; storage?: AccountStorage | null };
export type GeneratedLink = { url: string; expires_at: string };
export type MCPAccess = "read" | "read_write";
export type MCPGrant = { id: string; client_name: string; access: MCPAccess; created_at: string };
export type MCPGrants = { account_id: string; grants: MCPGrant[] };
export type MCPRevocation = { account_id: string };
export type MCPAuthorization = { username: string; client_name: string; redirect_host: string; access_options: MCPAccess[] };
export const accessLabel: Record<MCPAccess, string> = { read: "Read collection", read_write: "Read and edit collection" };
export type ErrorField = "username" | "password" | "token" | "current_password" | "new_password" | "confirmation";
export class ApiError extends Error {
  constructor(public code: string, public status = 0, public field: ErrorField | undefined = undefined) { super("Account request failed"); }
}
const fields: readonly string[] = ["username", "password", "token", "current_password", "new_password"];
export async function api<T>(path: string, body?: object, signal?: AbortSignal): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, { method: body === undefined ? "GET" : "POST", credentials: "same-origin", cache: "no-store", redirect: "error",
      ...(body === undefined ? {} : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }),
      ...(signal ? { signal } : {}) });
  } catch { throw new ApiError("connection_lost"); }
  return responseData<T>(response);
}
async function responseData<T>(response: Response): Promise<T> {
  let value: { data?: T; error?: {code?: unknown; field?: unknown} };
  try { value = await response.json(); } catch { throw new ApiError("invalid_response", response.status); }
  if (!response.ok) {
    const field = value?.error?.field;
    throw new ApiError(typeof value?.error?.code === "string" ? value.error.code : "invalid_response", response.status,
      typeof field === "string" && fields.includes(field) ? field as ErrorField : undefined);
  }
  if (!value || !Object.hasOwn(value, "data")) throw new ApiError("invalid_response", response.status);
  return value.data as T;
}
export function asError(error: unknown): ApiError { return error instanceof ApiError ? error : new ApiError("internal_error"); }
export function errorMessage(error: ApiError, claim = false): string {
  const messages: Record<string, string> = {
    authentication_failed: "Sign-in could not be verified. Check your credentials and sign in again.",
    permission_denied: "This action requires an administrator. Return to Connection or ask your administrator.",
    origin_rejected: "This server rejected the page origin. Open its configured HTTPS address and try again.",
    capability_invalid: "This link or setup token is unavailable. It may have expired or already been used. Try signing in, or ask the operator for a new link.",
    username_unavailable: "That username is unavailable. Choose another username and submit again.",
    claim_in_progress: "This claim is reserved with earlier credentials. Submit the original username and password, or ask the operator for help.",
    sole_administrator: "The sole enabled, ready administrator cannot be disabled or reset this way. Use password settings or local operator recovery.",
    stale_authority: "Your permission changed while this request was running. Sign in again before continuing.",
    validation_failed: "Check the marked field and submit again.",
    rate_limited: "Too many attempts. Wait at least five minutes before submitting again.",
    provision_unavailable: "Account storage is not ready. Keep the same credentials for an explicit retry, or contact the operator.",
    storage_unavailable: "Storage is unavailable. The result is not confirmed. Contact the operator before trying again.",
    admission_closed: "This server is not accepting account changes. Contact the operator.",
    invalid_request: "The request could not be read. Reload the page before submitting again.",
    unsupported_content_type: "This page and server are incompatible. Reload or contact the operator.",
    request_too_large: "The request is too large. Check the field lengths.",
    route_not_found: "This operation is unavailable. Return to Connection or contact the operator.",
    internal_error: "The server could not complete the request. Contact the operator.",
    invalid_response: "The server response could not be confirmed. Contact the operator before trying again.",
    connection_lost: "The response was lost. No further request was sent automatically."
  };
  const message = messages[error.code] ?? messages.internal_error!;
  return message + (claim && ["connection_lost", "invalid_response", "storage_unavailable", "internal_error"].includes(error.code)
    ? " The claim may already have completed. An explicit retry can be unavailable after completion. Try signing in with the submitted credentials before asking for another link." : "");
}
