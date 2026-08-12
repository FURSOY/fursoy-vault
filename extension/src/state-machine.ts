export type ControllerGroupState = "uninitialized" | "sealed" | "unlocking" | "leased" | "evicting" | "degraded";

export type StartupDecision =
  | { action: "none" }
  | { action: "invalidate" }
  | { action: "evict"; reason: "startup_reconciliation" | "last_tab_closed" | "expiry" }
  | { action: "schedule_expiry"; when: number }
  | { action: "clean_sealed" };

export function decideStartup(input: {
  state: ControllerGroupState;
  cookieCount: number;
  relevantTabCount: number;
  leaseExpiry?: number | null;
  reconciliationRequired: boolean;
  pendingInvalidation: boolean;
  now: number;
}): StartupDecision {
  if (input.pendingInvalidation && input.state !== "uninitialized") return { action: "invalidate" };
  if (input.state === "leased" && input.cookieCount === 0) return { action: "evict", reason: "startup_reconciliation" };
  if (input.state === "leased" && input.relevantTabCount === 0) return { action: "evict", reason: "last_tab_closed" };
  if (input.state === "leased" && typeof input.leaseExpiry === "number") {
    return input.leaseExpiry <= input.now
      ? { action: "evict", reason: "expiry" }
      : { action: "schedule_expiry", when: input.leaseExpiry };
  }
  if (input.state === "sealed" && input.cookieCount > 0) return { action: "clean_sealed" };
  if (input.reconciliationRequired) return { action: "evict", reason: "startup_reconciliation" };
  return { action: "none" };
}

export function stateAfterHostError(state: ControllerGroupState, pendingLease?: "inject" | "enroll"): { state: ControllerGroupState; reconciliation: boolean } {
  if (pendingLease === "inject") return { state: "sealed", reconciliation: false };
  if (pendingLease === "enroll" || state === "unlocking" || state === "evicting") return { state: "degraded", reconciliation: true };
  return { state, reconciliation: state === "degraded" };
}

export function stateAfterDisconnect(state: ControllerGroupState): { state: ControllerGroupState; reconciliation: boolean; activeLease: boolean } {
  if (state === "leased" || state === "evicting") {
    return { state: "degraded", reconciliation: true, activeLease: true };
  }
  return { state, reconciliation: state === "degraded", activeLease: false };
}

export function chainedEvictionAfterCompletion(
  operationKind: "enrollment" | "eviction" | "reconciliation",
  success: boolean,
  pendingReason: string | undefined,
): string | undefined {
  return success && operationKind === "enrollment" ? pendingReason : undefined;
}

export function shouldRetryReconciliation(success: boolean, attempts: number, limit = 2): boolean {
  return !success && Number.isSafeInteger(attempts) && attempts > 0 && attempts <= limit;
}
