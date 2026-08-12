export const COOKIE_CHUNK_TARGET_BYTES = 400 * 1024;
export const MAX_COOKIE_CHUNKS = 65_536;
export const MAX_COOKIE_RECORDS = 100_000;

interface PendingChunks<T> {
  leaseId: string;
  chunkCount: number;
  recordCount: number;
  nextChunk: number;
  records: T[];
}

/**
 * Connection-agnostic ordered chunk assembler preserving the current runtime semantics.
 * Workstream 2 can add connection-generation binding here without requiring Chrome globals.
 */
export class OrderedChunkAssembler<T> {
  private readonly pending = new Map<string, PendingChunks<T>>();

  receive(
    key: string,
    leaseId: string,
    chunkIndex: number,
    chunkCount: number,
    recordCount: number,
    records: T[],
  ): T[] | undefined {
    if (chunkCount < 1 || chunkCount > MAX_COOKIE_CHUNKS || recordCount < 0 || recordCount > MAX_COOKIE_RECORDS) {
      throw new Error("inject chunk metadata is out of range");
    }
    let current = this.pending.get(key);
    if (current === undefined) {
      if (chunkIndex !== 0) throw new Error("first inject chunk is missing");
      current = { leaseId, chunkCount, recordCount, nextChunk: 0, records: [] };
      this.pending.set(key, current);
    }
    if (current.leaseId !== leaseId || current.chunkCount !== chunkCount || current.recordCount !== recordCount || current.nextChunk !== chunkIndex) {
      this.pending.delete(key);
      throw new Error("inject chunk binding or order mismatch");
    }
    if (chunkIndex + 1 < chunkCount && records.length === 0) throw new Error("non-final inject chunk is empty");
    if (current.records.length + records.length > recordCount) throw new Error("inject cookie total exceeds declaration");
    current.records.push(...records);
    current.nextChunk += 1;
    if (current.nextChunk < chunkCount) return undefined;
    this.pending.delete(key);
    if (current.records.length !== recordCount) throw new Error("inject cookie total does not match declaration");
    return current.records;
  }

  /** Harness visibility that does not expose buffered record values. */
  pendingTransferCount(): number {
    return this.pending.size;
  }
}

export function chunkRecords<T>(records: T[]): T[][] {
  if (records.length === 0) return [[]];
  const encoder = new TextEncoder();
  const chunks: T[][] = [];
  let current: T[] = [];
  // Include the JSON array brackets, matching the production framing estimate used previously.
  let currentBytes = 2;
  for (const record of records) {
    const bytes = encoder.encode(JSON.stringify(record)).byteLength + (current.length === 0 ? 0 : 1);
    if (bytes > COOKIE_CHUNK_TARGET_BYTES) throw new Error("one cookie exceeds the chunk byte limit");
    if (current.length > 0 && currentBytes + bytes > COOKIE_CHUNK_TARGET_BYTES) {
      chunks.push(current);
      current = [];
      currentBytes = 2;
    }
    current.push(record);
    currentBytes += bytes;
  }
  chunks.push(current);
  return chunks;
}
