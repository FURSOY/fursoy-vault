export const HOST_NAME = "com.fursoy.vault";
export const PROTOCOL_VERSION = 3;

export type PolicyLevel = "critical" | "balanced" | "convenient" | "monitor";

export interface AccountGroup {
  id: string;
  display_name: string;
  scope: string;
  policy_level: PolicyLevel;
  eviction_triggers: string[];
  store_policy: "normal_profile";
}

export interface AccountGroupsConfig {
  version: number;
  compatibility_version: number;
  groups: AccountGroup[];
}

export interface LoadedConfig {
  config: AccountGroupsConfig;
  digest: string;
}

export interface PolicyParameters {
  leaseDurationMs: number;
  idleThresholdSeconds: number;
  lastTabGraceMs: number;
  monitoringOnly: boolean;
}

export function policyParameters(level: PolicyLevel): PolicyParameters {
  switch (level) {
    case "critical": return { leaseDurationMs: 300_000, idleThresholdSeconds: 60, lastTabGraceMs: 0, monitoringOnly: false };
    case "balanced": return { leaseDurationMs: 600_000, idleThresholdSeconds: 300, lastTabGraceMs: 120_000, monitoringOnly: false };
    case "convenient": return { leaseDurationMs: 1_800_000, idleThresholdSeconds: 900, lastTabGraceMs: 300_000, monitoringOnly: false };
    case "monitor": return { leaseDurationMs: 0, idleThresholdSeconds: 0, lastTabGraceMs: 0, monitoringOnly: true };
  }
}

// Q24: the host owns the config. The extension ships none of its own and validates whatever it
// is handed, so a malformed or tampered config stops the extension rather than being trusted.
export function validateConfig(config: AccountGroupsConfig): void {
  if (config.version !== 2 || config.compatibility_version !== 2 || !Array.isArray(config.groups) || config.groups.length < 1 || config.groups.length > 32) {
    throw new Error("unsupported account-group config");
  }
  const groupIds = new Set<string>();
  const scopes = new Set<string>();
  for (const group of config.groups) {
    if (!isUuid(group.id) || groupIds.has(group.id) || group.display_name.trim() === "") throw new Error("invalid account-group identity");
    groupIds.add(group.id);
    const scope = normalizeScope(group.scope);
    if (!isValidScope(scope) || scopes.has(scope)) throw new Error("invalid or duplicated account-group scope");
    // A scope that is a suffix of another would make cookie ownership ambiguous.
    for (const existing of scopes) {
      if (scope.endsWith(`.${existing}`) || existing.endsWith(`.${scope}`)) throw new Error("account-group scopes overlap");
    }
    scopes.add(scope);
    policyParameters(group.policy_level);
  }
}

function isValidScope(scope: string): boolean {
  if (scope === "" || scope.length > 253 || scope.startsWith(".") || scope.endsWith(".")) return false;
  if (!/^[a-z0-9.-]+$/.test(scope) || scope.includes("..")) return false;
  // A single label is not a registrable domain. Without this, internal page hostnames such as
  // `newtab` (from chrome://newtab/) would validate as a protectable scope.
  return scope.includes(".") || scope === "localhost";
}

export interface CookieRecord {
  domain: string; expiration_date?: number | null; host_only: boolean; http_only: boolean;
  name: string; partition_key?: { top_level_site?: string | null; has_cross_site_ancestor?: boolean | null } | null;
  path: string; same_site: chrome.cookies.SameSiteStatus; secure: boolean; session: boolean;
  store_id: string; value: string;
}

export interface WireMessage { type: string; payload: Record<string, unknown> }
export interface Envelope extends WireMessage {
  v: number; conn_nonce: string; seq: number; id: string;
}

export type CookieSetFailureCategory =
  | "permission" | "domain" | "samesite" | "secure" | "path" | "partition_key"
  | "store" | "url" | "invalid_cookie" | "unknown";

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

// ADR-015: the URL authorizing a cookie operation carries no port; it is rebuilt from the cookie's
// own scheme-equivalent (secure flag), host and path so no per-site URL table is needed.
export function cookieUrl(cookie: Pick<CookieRecord, "domain" | "path" | "secure"> | Pick<chrome.cookies.Cookie, "domain" | "path" | "secure">): string {
  const host = normalizeCookieDomain(cookie.domain);
  return `${cookie.secure ? "https" : "http"}://${host}${cookie.path.startsWith("/") ? cookie.path : "/"}`;
}

export function cookieSetDetails(group: AccountGroup, cookie: CookieRecord): chrome.cookies.SetDetails {
  if (!cookieBelongsToGroup(group, cookie)) throw new Error("cookie is outside the account-group scope");
  const details: chrome.cookies.SetDetails = {
    url: cookieUrl(cookie), name: cookie.name, value: cookie.value, path: cookie.path,
    secure: cookie.secure, httpOnly: cookie.http_only, sameSite: cookie.same_site,
    storeId: cookie.store_id,
  };
  if (!cookie.host_only) details.domain = cookie.domain;
  if (!cookie.session && typeof cookie.expiration_date === "number") details.expirationDate = cookie.expiration_date;
  if (typeof cookie.partition_key?.top_level_site === "string") {
    details.partitionKey = { topLevelSite: cookie.partition_key.top_level_site };
    if (typeof cookie.partition_key.has_cross_site_ancestor === "boolean") details.partitionKey.hasCrossSiteAncestor = cookie.partition_key.has_cross_site_ancestor;
  }
  return details;
}

export function hostInScope(scope: string, rawHost: string): boolean {
  const host = normalizeCookieDomain(rawHost);
  const normalized = normalizeScope(scope);
  return host === normalized || host.endsWith(`.${normalized}`);
}

export function cookieBelongsToGroup(group: AccountGroup, cookie: Pick<chrome.cookies.Cookie, "domain"> | Pick<CookieRecord, "domain">): boolean {
  return hostInScope(group.scope, cookie.domain);
}

export function groupForCookie(config: AccountGroupsConfig, cookie: Pick<chrome.cookies.Cookie, "domain">): AccountGroup | undefined {
  return config.groups.find((group) => cookieBelongsToGroup(group, cookie));
}

export function groupForUrl(config: AccountGroupsConfig, rawUrl: string | undefined): AccountGroup | undefined {
  if (rawUrl === undefined) return undefined;
  try {
    const url = new URL(rawUrl);
    if (url.protocol !== "http:" && url.protocol !== "https:") return undefined;
    return config.groups.find((group) => hostInScope(group.scope, url.hostname));
  } catch { return undefined; }
}

// chrome.tabs.query match patterns for the group, derived from the scope instead of a config list.
export function navigationPatterns(group: AccountGroup): string[] {
  const scope = normalizeScope(group.scope);
  return [`*://${scope}/*`, `*://*.${scope}/*`];
}

// NUL cannot occur in a cookie name, domain, path or store id, so it is an unambiguous joiner.
export const FIELD_SEPARATOR = String.fromCharCode(0);

export function cookieIdentity(cookie: Pick<chrome.cookies.Cookie, "name" | "domain" | "path" | "storeId" | "partitionKey"> | Pick<CookieRecord, "name" | "domain" | "path" | "store_id" | "partition_key">): string {
  const storeId = "storeId" in cookie ? cookie.storeId : cookie.store_id;
  const topLevelSite = "storeId" in cookie ? cookie.partitionKey?.topLevelSite ?? "" : cookie.partition_key?.top_level_site ?? "";
  return [cookie.name, normalizeCookieDomain(cookie.domain), cookie.path, storeId, topLevelSite].join(FIELD_SEPARATOR);
}

function normalizeCookieDomain(domain: string): string { return domain.replace(/^\./, "").toLowerCase(); }
function normalizeScope(scope: string): string { return scope.replace(/^\./, "").toLowerCase(); }
function isUuid(value: string): boolean { return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value); }

// Registrable-domain guess for a hostname. A full Public Suffix List is deliberately not
// bundled; the popup shows this guess in an editable field so the user corrects the cases a
// heuristic cannot know (ADR-020 slice 2).
const TWO_LABEL_SUFFIXES = new Set([
  "co.uk", "org.uk", "ac.uk", "gov.uk", "com.tr", "net.tr", "org.tr", "edu.tr", "gov.tr",
  "com.au", "net.au", "org.au", "co.nz", "co.jp", "co.kr", "com.br", "com.mx", "co.in",
  "com.cn", "com.sg", "co.za",
]);

export function guessScope(hostname: string): string {
  const host = hostname.trim().replace(/\.$/, "").toLowerCase();
  if (host === "" || host === "localhost") return host;
  // A bare IP address has no registrable domain; protect it exactly as given.
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.includes(":")) return host;
  const labels = host.split(".");
  if (labels.length <= 2) return host;
  const lastTwo = labels.slice(-2).join(".");
  return TWO_LABEL_SUFFIXES.has(lastTwo) ? labels.slice(-3).join(".") : lastTwo;
}
