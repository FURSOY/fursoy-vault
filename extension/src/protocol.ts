export const HOST_NAME = "com.fursoy.cookie_protector";
export const PROTOCOL_VERSION = 3;

export type PolicyLevel = "critical" | "balanced" | "convenient" | "monitor";
export type HealthCheckKind = "wikipedia_userinfo" | "json_session_state";

export interface CookieSelector {
  id: string;
  name: string;
  domain: string;
  path: string;
  required_for_enrollment: boolean;
  url: string;
}

export interface HealthCheck {
  kind: HealthCheckKind;
  origin: string;
  path: string;
}

export interface AccountGroup {
  id: string;
  display_name: string;
  domains: string[];
  navigation_patterns: string[];
  cookie_selectors: CookieSelector[];
  policy_level: PolicyLevel;
  eviction_triggers: string[];
  health_check: HealthCheck;
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

export async function loadAccountGroupsConfig(): Promise<LoadedConfig> {
  const response = await fetch(chrome.runtime.getURL("account-groups.json"), { cache: "no-store" });
  if (!response.ok) throw new Error("account-group config could not be loaded");
  const bytes = new Uint8Array(await response.arrayBuffer());
  const config = JSON.parse(new TextDecoder().decode(bytes)) as AccountGroupsConfig;
  validateConfig(config);
  const digest = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return { config, digest };
}

function validateConfig(config: AccountGroupsConfig): void {
  if (config.version !== 1 || config.compatibility_version !== 1 || !Array.isArray(config.groups) || config.groups.length < 1 || config.groups.length > 32) {
    throw new Error("unsupported account-group config");
  }
  const groupIds = new Set<string>();
  const selectorOwners = new Set<string>();
  const patternOwners = new Set<string>();
  let selectorCount = 0;
  for (const group of config.groups) {
    if (!isUuid(group.id) || groupIds.has(group.id) || group.display_name.trim() === "") throw new Error("invalid account-group identity");
    groupIds.add(group.id);
    if (!Array.isArray(group.domains) || !Array.isArray(group.navigation_patterns) || !Array.isArray(group.cookie_selectors) || group.cookie_selectors.length === 0) throw new Error("empty account-group routing data");
    selectorCount += group.cookie_selectors.length;
    for (const pattern of group.navigation_patterns) {
      if (patternOwners.has(pattern)) throw new Error("navigation pattern belongs to multiple groups");
      patternOwners.add(pattern);
    }
    const selectorIds = new Set<string>();
    for (const selector of group.cookie_selectors) {
      const identity = `${selector.name}\u0000${normalizeCookieDomain(selector.domain)}\u0000${selector.path}`;
      if (selectorIds.has(selector.id) || selectorOwners.has(identity) || selector.id === "" || selector.name === "" || !selector.path.startsWith("/")) throw new Error("invalid or overlapping cookie selector");
      selectorIds.add(selector.id);
      selectorOwners.add(identity);
    }
    if (!group.cookie_selectors.some((selector) => selector.required_for_enrollment)) throw new Error("group has no required enrollment selector");
    policyParameters(group.policy_level);
  }
  if (selectorCount > 256) throw new Error("cookie selector count exceeds limit");
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

export function cookieSetDetails(group: AccountGroup, cookie: CookieRecord): chrome.cookies.SetDetails {
  const selector = selectorForCookie(group, cookie);
  if (selector === undefined) throw new Error("cookie is outside the account-group selectors");
  const details: chrome.cookies.SetDetails = {
    url: selector.url, name: cookie.name, value: cookie.value, path: cookie.path,
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

export function selectorForCookie(group: AccountGroup, cookie: Pick<chrome.cookies.Cookie, "name" | "domain" | "path"> | Pick<CookieRecord, "name" | "domain" | "path">): CookieSelector | undefined {
  const domain = normalizeCookieDomain(cookie.domain);
  return group.cookie_selectors.find((selector) => selector.name === cookie.name && normalizeCookieDomain(selector.domain) === domain && selector.path === cookie.path);
}

export function groupForCookie(config: AccountGroupsConfig, cookie: Pick<chrome.cookies.Cookie, "name" | "domain" | "path">): AccountGroup | undefined {
  return config.groups.find((group) => selectorForCookie(group, cookie) !== undefined);
}

export function hasRequiredEnrollmentCookies(group: AccountGroup, cookies: readonly chrome.cookies.Cookie[]): boolean {
  return group.cookie_selectors.filter((selector) => selector.required_for_enrollment)
    .every((selector) => cookies.some((cookie) => selectorForCookie(group, cookie)?.id === selector.id));
}

export function cookieIdentity(cookie: Pick<chrome.cookies.Cookie, "name" | "domain" | "path" | "storeId" | "partitionKey"> | Pick<CookieRecord, "name" | "domain" | "path" | "store_id" | "partition_key">): string {
  const storeId = "storeId" in cookie ? cookie.storeId : cookie.store_id;
  const topLevelSite = "storeId" in cookie ? cookie.partitionKey?.topLevelSite ?? "" : cookie.partition_key?.top_level_site ?? "";
  return [cookie.name, normalizeCookieDomain(cookie.domain), cookie.path, storeId, topLevelSite].join("\u0000");
}

export function groupForUrl(config: AccountGroupsConfig, rawUrl: string | undefined): AccountGroup | undefined {
  if (rawUrl === undefined) return undefined;
  try {
    const url = new URL(rawUrl);
    return config.groups.find((group) =>
      group.navigation_patterns.some((pattern) => matchesPattern(pattern, url)) &&
      (group.health_check.kind !== "json_session_state" || url.origin === group.health_check.origin),
    );
  } catch { return undefined; }
}

function matchesPattern(pattern: string, url: URL): boolean {
  const match = /^(https?):\/\/([^/]+)\/\*$/.exec(pattern);
  if (match === null || `${match[1]}:` !== url.protocol) return false;
  const host = match[2]?.toLowerCase();
  if (host === undefined) return false;
  return host.startsWith("*.") ? url.hostname.toLowerCase().endsWith(host.slice(1)) : url.hostname.toLowerCase() === host;
}

function normalizeCookieDomain(domain: string): string { return domain.replace(/^\./, "").toLowerCase(); }
function isUuid(value: string): boolean { return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value); }
