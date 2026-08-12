export type OperationKind = "enrollment" | "eviction" | "reconciliation";
export type OperationPhase =
  | "begin_pending" | "snapshot_required" | "committed" | "removal_precheck"
  | "removal_authorized" | "completed" | "reconciliation_required";

export interface OperationReference {
  groupId: string;
  operationId?: string;
  operationSequence?: number;
  leaseId?: string;
  attemptId: string;
  kind: OperationKind;
  phase: OperationPhase;
}

export interface OperationReferenceStore {
  load(): Promise<unknown>;
  save(value: unknown): Promise<void>;
}

interface StoredOperationReferences {
  version: 2;
  operations: OperationReference[];
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export class OperationCoordinator {
  private active = new Map<string, OperationReference>();
  constructor(private readonly store: OperationReferenceStore) {}

  async restore(): Promise<OperationReference[]> {
    const stored = await this.store.load();
    const values = this.validateCollection(stored);
    this.active = new Map(values.map((value) => [value.groupId, value]));
    // Rewrites the old single-reference format as the per-group v2 collection. This metadata
    // contains no cookie values or snapshot payloads.
    await this.persist();
    return this.currents();
  }

  current(groupId: string): OperationReference | undefined {
    const value = this.active.get(groupId);
    return value === undefined ? undefined : { ...value };
  }

  currents(): OperationReference[] {
    return [...this.active.values()].sort((left, right) => left.groupId.localeCompare(right.groupId)).map((value) => ({ ...value }));
  }

  async begin(groupId: string, leaseId: string | undefined, kind: OperationKind): Promise<OperationReference> {
    // reconciliation_required is a terminal outcome for the old operation, not an in-flight
    // request that can safely be replayed. Reusing it traps the extension in a loop where every
    // reconnect queries the same terminal operation and can never issue a fresh reconciliation.
    const current = this.active.get(groupId);
    if (current !== undefined && current.phase !== "completed" && current.phase !== "reconciliation_required") {
      if (current.leaseId === leaseId && current.kind === kind) return { ...current };
      // A begin_pending reference has not learned a host operation identity yet. Reusing it with
      // a different lease/kind would put the new values on the wire while retaining the old local
      // binding, making the host's snapshot_required response impossible to accept. It is safe to
      // replace only this unissued reference; an issued operation must be reconciled by identity.
      if (current.phase !== "begin_pending") throw new Error("active operation binding changed");
    }
    const created: OperationReference = { groupId, leaseId, kind, attemptId: crypto.randomUUID(), phase: "begin_pending" };
    this.active.set(groupId, created);
    await this.persist();
    return { ...created };
  }

  async bindIssued(groupId: string, operationId: string, operationSequence: number, leaseId: string | undefined, attemptId: string): Promise<OperationReference> {
    const active = this.required(groupId);
    if (active.attemptId !== attemptId || active.leaseId !== leaseId || !UUID.test(operationId)
      || !Number.isSafeInteger(operationSequence) || operationSequence < 1) throw new Error("operation issue binding mismatch");
    const issued: OperationReference = { ...active, operationId, operationSequence, phase: "snapshot_required" };
    this.active.set(groupId, issued);
    await this.persist();
    return { ...issued };
  }

  async phase(groupId: string, phase: OperationPhase): Promise<void> {
    this.active.set(groupId, { ...this.required(groupId), phase });
    await this.persist();
  }

  statusQuery(groupId: string): Record<string, unknown> | undefined {
    const value = this.active.get(groupId);
    if (value?.operationId === undefined || value.operationSequence === undefined ||
        value.phase === "completed" || value.phase === "reconciliation_required") return undefined;
    return { account_group_id: value.groupId, operation_id: value.operationId,
      operation_sequence: value.operationSequence, lease_id: value.leaseId ?? null };
  }

  recoveryQuery(groupId: string): Record<string, unknown> | undefined {
    const value = this.active.get(groupId);
    if (value === undefined || value.phase === "completed" || value.phase === "reconciliation_required") return undefined;
    // begin_pending has no host identity and must use the idempotent begin retry path. Once an
    // identity exists, that durable operation must be resumed even if a startup trigger proposes
    // a different kind or observes a repaired lease projection.
    return this.statusQuery(groupId);
  }

  statusQueries(): Record<string, unknown>[] {
    return this.currents().map((value) => this.statusQuery(value.groupId)).filter((value): value is Record<string, unknown> => value !== undefined);
  }

  assertBinding(payload: Record<string, unknown>): OperationReference {
    const groupId = String(payload.account_group_id ?? "");
    const active = this.required(groupId);
    if (payload.operation_id !== active.operationId || payload.operation_sequence !== active.operationSequence
      || (payload.lease_id ?? undefined) !== active.leaseId
      || (payload.attempt_id !== undefined && payload.attempt_id !== active.attemptId)) {
      throw new Error("operation response binding mismatch");
    }
    return active;
  }

  async complete(groupId: string): Promise<void> {
    this.required(groupId);
    this.active.delete(groupId);
    await this.persist();
  }

  async discardGroup(groupId: string): Promise<void> {
    if (!this.active.delete(groupId)) return;
    await this.persist();
  }

  private required(groupId: string): OperationReference {
    const value = this.active.get(groupId);
    if (value === undefined) throw new Error("no matching active operation");
    return value;
  }

  private async persist(): Promise<void> {
    const operations = this.currents();
    await this.store.save(operations.length === 0 ? undefined : { version: 2, operations } satisfies StoredOperationReferences);
  }

  private validateCollection(value: unknown): OperationReference[] {
    const legacy = this.validateStored(value);
    if (legacy !== undefined) return [legacy];
    const collection = value as Partial<StoredOperationReferences> | undefined;
    if (collection?.version !== 2 || !Array.isArray(collection.operations) || collection.operations.length > 32) return [];
    const operations = collection.operations.map((item) => this.validateStored(item));
    if (operations.some((item) => item === undefined)) return [];
    const valid = operations as OperationReference[];
    if (new Set(valid.map((item) => item.groupId)).size !== valid.length) return [];
    return valid;
  }

  private validateStored(value: unknown): OperationReference | undefined {
    const item = value as Partial<OperationReference> | undefined;
    if (item === undefined || typeof item !== "object" || !UUID.test(item.groupId ?? "")
      || !UUID.test(item.attemptId ?? "") || !["enrollment", "eviction", "reconciliation"].includes(item.kind ?? "")
      || !["begin_pending", "snapshot_required", "committed", "removal_precheck", "removal_authorized", "completed", "reconciliation_required"].includes(item.phase ?? "")) return undefined;
    if (item.operationId !== undefined && !UUID.test(item.operationId)) return undefined;
    if (item.operationSequence !== undefined && (!Number.isSafeInteger(item.operationSequence) || item.operationSequence < 1)) return undefined;
    return item as OperationReference;
  }
}

// If Chrome cannot observe a newly enrolled scope, no vault object exists yet and committing an
// empty snapshot would be wrong. Sending an empty enrollment snapshot is instead the protocol's
// explicit scope-empty abort path. Existing vault operations must never use that fallback.
export function mayAbortWithEmptySnapshot(kind: OperationKind): boolean {
  return kind === "enrollment";
}
