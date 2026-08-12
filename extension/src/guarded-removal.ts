import { cookieIdentity, type CookieRecord } from "./protocol.js";

function exactRecord(record: CookieRecord): string {
  return JSON.stringify(record);
}

export function snapshotsEqual(expected: readonly CookieRecord[], actual: readonly CookieRecord[]): boolean {
  if (expected.length !== actual.length) return false;
  const right = new Map(actual.map((record) => [cookieIdentity(record), exactRecord(record)]));
  return expected.every((record) => right.get(cookieIdentity(record)) === exactRecord(record));
}

export class GuardedRemovalPlan {
  private nextIndex = 0;
  constructor(private readonly authorized: readonly CookieRecord[]) {}

  next(current: readonly CookieRecord[]): { done: true } | { done: false; record: CookieRecord } | { mutation: true } {
    const expectedRemaining = this.authorized.slice(this.nextIndex);
    const expected = new Map(expectedRemaining.map((record) => [cookieIdentity(record), exactRecord(record)]));
    // Chrome may remove/expire another already-authorized cookie as a side effect of deleting
    // one cookie. That only narrows exposure and the committed vault still contains the value,
    // so it is safe. A new identity or changed value is not safe and still aborts removal.
    if (current.some((record) => expected.get(cookieIdentity(record)) !== exactRecord(record))) return { mutation: true };
    while (this.nextIndex < this.authorized.length) {
      const candidate = this.authorized[this.nextIndex++]!;
      if (current.some((record) => cookieIdentity(record) === cookieIdentity(candidate))) {
        return { done: false, record: candidate };
      }
    }
    return current.length === 0 ? { done: true } : { mutation: true };
  }
}
