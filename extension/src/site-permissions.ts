export function permissionOrigins(scopes: readonly string[]): string[] {
  return [...new Set(scopes.flatMap((scope) => [
    `*://${scope}/*`,
    `*://*.${scope}/*`,
  ]))];
}

export function shouldOfferGrantAll(missingPermissionCount: number): boolean {
  return Number.isSafeInteger(missingPermissionCount) && missingPermissionCount > 1;
}
