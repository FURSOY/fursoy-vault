export const GROUP_ID = "7a144677-3f5c-4a86-a767-16fd3ca315b8";
export const HOST_NAME = "com.fursoy.cookie_protector";
export const ORIGIN = "https://tr.wikipedia.org";

export interface CookieSelector {
  id: string;
  name: string;
  domain: string;
  path: string;
  requiredForEnrollment: boolean;
  url: string;
}

// MediaWiki documents <wikiID>Session/UserID/UserName and an optional Token. Wikimedia's
// CentralAuth adds Session/User and an optional Token on the parent domain. Exact selectors keep
// unrelated preference, analytics, anti-abuse, and logout-marker cookies outside the vault.
export const COOKIE_SELECTORS: readonly CookieSelector[] = [
  { id: "local_session", name: "trwikiSession", domain: "tr.wikipedia.org", path: "/", requiredForEnrollment: true, url: `${ORIGIN}/` },
  { id: "local_user_id", name: "trwikiUserID", domain: "tr.wikipedia.org", path: "/", requiredForEnrollment: true, url: `${ORIGIN}/` },
  { id: "local_user_name", name: "trwikiUserName", domain: "tr.wikipedia.org", path: "/", requiredForEnrollment: true, url: `${ORIGIN}/` },
  { id: "local_token", name: "trwikiToken", domain: "tr.wikipedia.org", path: "/", requiredForEnrollment: false, url: `${ORIGIN}/` },
  { id: "central_session", name: "centralauth_Session", domain: "wikipedia.org", path: "/", requiredForEnrollment: true, url: `${ORIGIN}/` },
  { id: "central_user", name: "centralauth_User", domain: "wikipedia.org", path: "/", requiredForEnrollment: true, url: `${ORIGIN}/` },
  { id: "central_token", name: "centralauth_Token", domain: "wikipedia.org", path: "/", requiredForEnrollment: false, url: `${ORIGIN}/` },
] as const;

export interface CookieRecord {
  domain: string; expiration_date?: number | null; host_only: boolean; http_only: boolean;
  name: string; partition_key?: { top_level_site?: string | null; has_cross_site_ancestor?: boolean | null } | null;
  path: string; same_site: chrome.cookies.SameSiteStatus; secure: boolean; session: boolean;
  store_id: string; value: string;
}

export interface WireMessage { type: string; payload: Record<string, unknown> }
export interface Envelope extends WireMessage {
  v: 1; conn_nonce: string; seq: number; id: string;
}

export type CookieSetFailureCategory =
  | "permission"
  | "domain"
  | "samesite"
  | "secure"
  | "path"
  | "partition_key"
  | "store"
  | "url"
  | "invalid_cookie"
  | "unknown";

// Convert Chrome's potentially identifying error text into a fixed, non-secret vocabulary. The
// caller must discard the original string immediately and persist/log only this return value.
export function categorizeCookieSetFailure(message: string | undefined): CookieSetFailureCategory {
  if (message === undefined || message.trim() === "") return "unknown";
  const normalized = message.toLowerCase();
  if (/(permission|not permitted|host access|access denied|not allowed|not authorized)/.test(normalized)) return "permission";
  if (/(partition|top.?level.?site|cross.?site.?ancestor)/.test(normalized)) return "partition_key";
  if (/(same.?site)/.test(normalized)) return "samesite";
  if (/(secure|insecure)/.test(normalized)) return "secure";
  if (/(domain|host.?only)/.test(normalized)) return "domain";
  if (/(path)/.test(normalized)) return "path";
  if (/(store.?id|cookie store|incognito)/.test(normalized)) return "store";
  if (/(url|scheme|origin)/.test(normalized)) return "url";
  if (/(parse|invalid|malformed|cookie)/.test(normalized)) return "invalid_cookie";
  return "unknown";
}

export function cookieRecord(cookie: chrome.cookies.Cookie): CookieRecord {
  const record: CookieRecord = {
    domain: cookie.domain, host_only: cookie.hostOnly, http_only: cookie.httpOnly,
    name: cookie.name, path: cookie.path, same_site: cookie.sameSite, secure: cookie.secure,
    session: cookie.session, store_id: cookie.storeId, value: cookie.value,
  };
  if (cookie.expirationDate !== undefined) record.expiration_date = cookie.expirationDate;
  if (cookie.partitionKey !== undefined) record.partition_key = {
    top_level_site: cookie.partitionKey.topLevelSite,
    has_cross_site_ancestor: cookie.partitionKey.hasCrossSiteAncestor,
  };
  return record;
}

export function cookieSetDetails(cookie: CookieRecord): chrome.cookies.SetDetails {
  const selector = selectorForCookie(cookie);
  if (selector === undefined) throw new Error("cookie is outside the account-group selectors");
  const details: chrome.cookies.SetDetails = {
    url: selector.url,
    name: cookie.name,
    value: cookie.value,
    path: cookie.path,
    secure: cookie.secure,
    httpOnly: cookie.http_only,
    sameSite: cookie.same_site,
    storeId: cookie.store_id,
  };
  if (!cookie.host_only) details.domain = cookie.domain;
  if (!cookie.session && typeof cookie.expiration_date === "number") {
    details.expirationDate = cookie.expiration_date;
  }
  if (typeof cookie.partition_key?.top_level_site === "string") {
    details.partitionKey = { topLevelSite: cookie.partition_key.top_level_site };
    if (typeof cookie.partition_key.has_cross_site_ancestor === "boolean") {
      details.partitionKey.hasCrossSiteAncestor = cookie.partition_key.has_cross_site_ancestor;
    }
  }
  return details;
}

export function selectorForCookie(cookie: Pick<chrome.cookies.Cookie, "name" | "domain" | "path"> | Pick<CookieRecord, "name" | "domain" | "path">): CookieSelector | undefined {
  const domain = normalizeCookieDomain(cookie.domain);
  return COOKIE_SELECTORS.find((selector) =>
    selector.name === cookie.name && selector.domain === domain && selector.path === cookie.path,
  );
}

export function isGroupCookie(cookie: Pick<chrome.cookies.Cookie, "name" | "domain" | "path">): boolean {
  return selectorForCookie(cookie) !== undefined;
}

export function hasRequiredEnrollmentCookies(cookies: readonly chrome.cookies.Cookie[]): boolean {
  return COOKIE_SELECTORS
    .filter((selector) => selector.requiredForEnrollment)
    .every((selector) => cookies.some((cookie) => selectorForCookie(cookie)?.id === selector.id));
}

export function cookieIdentity(cookie: Pick<chrome.cookies.Cookie, "name" | "domain" | "path" | "storeId" | "partitionKey"> | Pick<CookieRecord, "name" | "domain" | "path" | "store_id" | "partition_key">): string {
  const storeId = "storeId" in cookie ? cookie.storeId : cookie.store_id;
  const topLevelSite = "storeId" in cookie
    ? cookie.partitionKey?.topLevelSite ?? ""
    : cookie.partition_key?.top_level_site ?? "";
  return [cookie.name, normalizeCookieDomain(cookie.domain), cookie.path, storeId, topLevelSite].join("\u0000");
}

function normalizeCookieDomain(domain: string): string {
  return domain.replace(/^\./, "").toLowerCase();
}
