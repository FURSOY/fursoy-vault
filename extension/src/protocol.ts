export const HOST_NAME = "com.fursoy.vault";
export const PROTOCOL_VERSION = 7;
export const EXTENSION_VERSION = "0.5.5";
export const MIN_HOST_VERSION = "0.5.0";
export const REQUIRED_CAPABILITIES = ["chunked_cookies", "request_correlation", "config_v3", "audit_recovery", "profile_namespace", "durable_operations_v7", "guarded_cookie_removal", "semantic_operation_status", "profile_recovery_v1"] as const;

export function compareSemanticVersions(left: string, right: string): number {
  const parse = (value: string): number[] => {
    if (!/^\d+\.\d+\.\d+$/.test(value)) throw new Error("invalid semantic version");
    return value.split(".").map((part) => Number(part));
  };
  const [a, b] = [parse(left), parse(right)];
  for (let index = 0; index < 3; index += 1) {
    const difference = a[index]! - b[index]!;
    if (difference !== 0) return difference;
  }
  return 0;
}

export type PolicyLevel = "critical" | "balanced" | "convenient" | "monitor";

export interface AccountGroup {
  id: string;
  display_name: string;
  scope: string;
  policy_level: PolicyLevel;
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
    case "convenient": return { leaseDurationMs: 14_400_000, idleThresholdSeconds: 3_600, lastTabGraceMs: 900_000, monitoringOnly: false };
    case "monitor": return { leaseDurationMs: 0, idleThresholdSeconds: 0, lastTabGraceMs: 0, monitoringOnly: true };
  }
}

// Q24: the host owns the config. The extension ships none of its own and validates whatever it
// is handed, so a malformed or tampered config stops the extension rather than being trusted.
export function validateConfig(config: AccountGroupsConfig): void {
  // Zero groups is valid pre-launch (2026-08-08, ADR-024): a fresh install starts empty and the
  // user adds their first group through onboarding — mirrors the host-side relaxation in
  // native-host/src/config.rs.
  if (config.version !== 3 || config.compatibility_version !== 3 || !Array.isArray(config.groups) || config.groups.length > 32) {
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

export function isValidProtectionScope(value: string): boolean {
  const scope = value.trim().replace(/^\./, "").toLowerCase();
  if (scope === "" || scope.length > 253 || scope.startsWith(".") || scope.endsWith(".")) return false;
  if (!/^[a-z0-9.-]+$/.test(scope) || scope.includes("..")) return false;
  if (scope === "localhost") return true;
  const parsed = parseDomain(scope, { allowPrivateDomains: true });
  if (parsed.isIp) return isValidIpv4(scope);
  // A protection scope is exactly one registrable domain. Bare public/private suffixes (co.uk,
  // github.io) and arbitrary subdomains are rejected so one group can never span unrelated sites.
  return parsed.domain === scope && (parsed.isIcann === true || parsed.isPrivate === true);
}

export function normalizeProtectionScopeInput(value: string): string {
  const host = value.trim().replace(/^https?:\/\//i, "").replace(/[\/?#].*$/, "").replace(/^\./, "").toLowerCase();
  if (host === "" || host === "localhost") return host;
  const parsed = parseDomain(host, { allowPrivateDomains: true });
  return parsed.isIp ? host : parsed.domain ?? host;
}

const isValidScope = isValidProtectionScope;

function isValidIpv4(scope: string): boolean {
  const parts = scope.split(".");
  return parts.length === 4 && parts.every((part) => /^(0|[1-9]\d{0,2})$/.test(part) && Number(part) <= 255);
}

export interface CookieRecord {
  domain: string; expiration_date?: number | null; host_only: boolean; http_only: boolean;
  name: string; partition_key?: { top_level_site?: string | null; has_cross_site_ancestor?: boolean | null } | null;
  path: string; same_site: chrome.cookies.SameSiteStatus; secure: boolean; session: boolean;
  store_id: string; value: string;
}

export interface WireMessage { type: string; payload: Record<string, unknown>; requestId?: string }
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
  const hasCrossSiteAncestor = "storeId" in cookie ? cookie.partitionKey?.hasCrossSiteAncestor : cookie.partition_key?.has_cross_site_ancestor;
  return [cookie.name, normalizeCookieDomain(cookie.domain), cookie.path, storeId, normalizeTopLevelSite(topLevelSite), hasCrossSiteAncestor === true ? "1" : hasCrossSiteAncestor === false ? "0" : ""].join(FIELD_SEPARATOR);
}

export function cookieRoundTripMatches(expected: CookieRecord, actual: chrome.cookies.Cookie): boolean {
  if (cookieIdentity(expected) !== cookieIdentity(actual)) return false;
  if (expected.value !== actual.value || expected.host_only !== actual.hostOnly || expected.http_only !== actual.httpOnly) return false;
  if (expected.secure !== actual.secure || expected.session !== actual.session || expected.same_site !== actual.sameSite) return false;
  if (!expected.session) {
    if (typeof expected.expiration_date !== "number" || typeof actual.expirationDate !== "number") return false;
    // Chrome may normalize a persistent expiry to whole seconds. This tolerance accepts only that
    // representation change, not a materially different lifetime.
    if (Math.abs(expected.expiration_date - actual.expirationDate) > 1) return false;
  }
  return true;
}

function normalizeCookieDomain(domain: string): string { return domain.replace(/^\./, "").toLowerCase(); }
function normalizeTopLevelSite(site: string): string {
  if (site === "") return "";
  try { return new URL(site).origin.toLowerCase(); } catch { return site.toLowerCase().replace(/\/$/, ""); }
}
function normalizeScope(scope: string): string { return scope.replace(/^\./, "").toLowerCase(); }
function isUuid(value: string): boolean { return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value); }

export function guessScope(hostname: string): string {
  const host = hostname.trim().replace(/\.$/, "").toLowerCase();
  if (host === "" || host === "localhost") return host;
  const parsed = parseDomain(host, { allowPrivateDomains: true });
  if (parsed.isIp) return host;
  return parsed.domain ?? "";
}
import { parse as parseDomain } from "tldts";
