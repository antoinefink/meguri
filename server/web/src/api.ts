// SPDX-License-Identifier: MIT
export type Identity = { id: string; username: string; role: "admin" | "member" };
export type Me = Identity & { server_url: string };
export type AccountStorage = { collection_bytes: number; media_bytes: number; total_bytes: number };
export type CollectionBackup = { id: string; created_at: number; kind: "daily" | "weekly" | "monthly" | "before_restore"; size_bytes: number };
export type BackupHistory = { account_id: string; backups: CollectionBackup[] };
export type BackupRestore = { account_id: string };
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
export function parseBackupHistory(value: unknown, expectedAccount: string): CollectionBackup[] {
  if (!value || typeof value !== "object" || !("account_id" in value) || typeof value.account_id !== "string"
    || !value.account_id.length || !("backups" in value)) throw new ApiError("invalid_response");
  if (value.account_id !== expectedAccount) throw new ApiError("stale_authority");
  value = value.backups;
  const ids = new Set<string>();
  if (!Array.isArray(value) || !value.every(item => {
    if (!item || typeof item !== "object" || typeof item.id !== "string" || !item.id.length || ids.has(item.id)
      || !["daily", "weekly", "monthly", "before_restore"].includes(item.kind)
      || !Number.isInteger(item.created_at) || !Number.isFinite(new Date(item.created_at * 1000).getTime())
      || !Number.isSafeInteger(item.size_bytes) || item.size_bytes < 0) return false;
    ids.add(item.id); return true;
  })) throw new ApiError("invalid_response");
  return value;
}
const fields: readonly string[] = ["username", "password", "token", "current_password", "new_password"];
export async function api<T>(path: string, body?: object, signal?: AbortSignal, expectedAccount?: string): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, { method: body === undefined ? "GET" : "POST", credentials: "same-origin", cache: "no-store", redirect: "error",
      headers: { ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        ...(expectedAccount === undefined ? {} : { "X-Meguri-Expected-Account": expectedAccount }) },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
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
    busy: "The collection is busy syncing. Wait for sync to finish, then try this action again.",
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

// W12: string identities and account-bound browser collection operations.
export type BrowseFragment = { text: string; matched: boolean };
export type BrowseCard = { card_id: string; note_id: string; title: string; field: string | null; excerpt: BrowseFragment[]; deck: string; template: string; unavailable?: string };
export type BrowseNote = { note_id: string; note_type_id: string; fields: Record<string, string>; tags: string[]; revision: string };
export type BrowseDetail = { account_id: string; card: BrowseCard; note: BrowseNote; field_names: string[]; sibling_ids: string[]; confirmation: string };
export type BrowsePage = { account_id: string; cards: BrowseCard[]; next_cursor: string | null; examined_through: string | null };
export type BrowseDeck = { deck_id: string; name: string; filtered: boolean };
export type BrowseNoteType = { note_type_id: string; name: string; field_names: string[] };
export type BrowseCatalog = { account_id: string; generation: string; decks: BrowseDeck[]; note_types: BrowseNoteType[]; next_cursor: string | null };
export type BrowseWrite = { account_id: string; note_id: string; revision: string };
export type BrowseScope = { query: string; field: string | null; deck_id: string | null };
type BrowseResponses = { catalog: BrowseCatalog; page: BrowsePage; detail: BrowseDetail; create: BrowseWrite; edit: BrowseWrite; delete: { account_id: string } };
function record(value: unknown): value is Record<string, unknown> { return value !== null && typeof value === "object" && !Array.isArray(value); }
function strings(value: unknown): value is string[] { return Array.isArray(value) && value.every(item => typeof item === "string"); }
const exactId = (value: unknown): value is string => typeof value === "string" && /^[1-9][0-9]{0,18}$/.test(value);
function browseCard(value: unknown): value is BrowseCard {
  return record(value) && exactId(value.card_id) && exactId(value.note_id) && typeof value.title === "string"
    && (value.field === null || typeof value.field === "string") && typeof value.deck === "string" && typeof value.template === "string"
    && (value.unavailable === undefined || typeof value.unavailable === "string")
    && Array.isArray(value.excerpt) && value.excerpt.every(part => record(part) && typeof part.text === "string" && typeof part.matched === "boolean");
}
export function parseBrowse<K extends keyof BrowseResponses>(kind: K, value: unknown, accountId: string): BrowseResponses[K] {
  if (!record(value) || typeof value.account_id !== "string") throw new ApiError("invalid_response");
  if (value.account_id !== accountId) throw new ApiError("stale_authority");
  const cursor = value.next_cursor === null || typeof value.next_cursor === "string";
  let valid = false;
  if (kind === "catalog") valid = typeof value.generation === "string" && cursor && Array.isArray(value.decks)
    && value.decks.every(deck => record(deck) && exactId(deck.deck_id) && typeof deck.name === "string" && typeof deck.filtered === "boolean")
    && Array.isArray(value.note_types) && value.note_types.every(nt => record(nt) && exactId(nt.note_type_id) && typeof nt.name === "string" && strings(nt.field_names));
  if (kind === "page") valid = cursor && (value.examined_through === null || exactId(value.examined_through)) && Array.isArray(value.cards) && value.cards.length <= 50 && value.cards.every(browseCard);
  if (kind === "detail") valid = browseCard(value.card) && record(value.note) && value.card.note_id === value.note.note_id
    && exactId(value.note.note_id) && exactId(value.note.note_type_id) && typeof value.note.revision === "string"
    && strings(value.note.tags) && record(value.note.fields) && Object.values(value.note.fields).every(field => typeof field === "string")
    && strings(value.field_names) && value.field_names.every(name => Object.hasOwn((value.note as BrowseNote).fields, name))
    && strings(value.sibling_ids) && value.sibling_ids.every(exactId) && value.sibling_ids.includes(value.card.card_id)
    && typeof value.confirmation === "string";
  if (kind === "edit" || kind === "create") valid = exactId(value.note_id) && typeof value.revision === "string";
  if (kind === "delete") valid = true;
  if (!valid) throw new ApiError("invalid_response");
  return value as unknown as BrowseResponses[K];
}
export async function browseApi<K extends keyof BrowseResponses>(kind: K, body: object, accountId: string, signal?: AbortSignal): Promise<BrowseResponses[K]> {
  return parseBrowse(kind, await api<unknown>(`/api/me/browse/${kind}`, body, signal, accountId), accountId);
}
