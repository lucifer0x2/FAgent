/**
 * IndexedDB store of the engine's persistent frame records (Task 42), opened in the engine
 * Worker: database `fagent.engine-scenes`, separate from the Canvas2D `fagent.browser-scenes`.
 *
 * - `records`: one computed frame per `[build, projectId, digest, width, height, theme]`
 *   (never a revision): the record bytes (`export_record`), its fonts and assets.
 * - `usage`: the same keys with last use and size only, so eviction never loads records.
 * - `manifests`: frame digests per `[build, projectId, etag, framesKey]`, so a load of the
 *   same snapshot representation (strong ETag) and frame list skips `frame_keys`.
 *
 * Best effort: opening and every transaction have a hard deadline, and a timeout, quota
 * error, blocked or missing database reads as a miss (the caller computes).
 */
export const THEME = "default";
export const RECORD_ENTRIES = 2_000;
export const RECORD_BYTES = 256 * 1024 * 1024;
export const RECORD_ENTRY_BYTES = 16 * 1024 * 1024;
export const MANIFESTS = 32;
const OPEN_DEADLINE = 1_000;
/** One read for a whole board (~1,000 records); records arrive one by one before it ends. */
const READ_DEADLINE = 3_000;
const WRITE_DEADLINE = 10_000;
const NAME = "fagent.engine-scenes";

export type RecordKey = [build: string, projectId: string, digest: string, width: number, height: number, theme: string];
export type ManifestKey = [build: string, projectId: string, etag: string, framesKey: string];

export interface Resource {
  id: string;
  hash: string;
}

export interface StoredRecord {
  build: string;
  projectId: string;
  digest: string;
  width: number;
  height: number;
  theme: string;
  frameId: string;
  bytes: ArrayBuffer;
  fonts: Resource[];
  assets: Resource[];
  size: number;
}

export interface StoredManifest {
  build: string;
  projectId: string;
  etag: string;
  framesKey: string;
  digests: Record<string, string>;
  used: number;
}

export interface Usage {
  key: RecordKey;
  size: number;
  used: number;
}

export interface WriteBatch {
  records: StoredRecord[];
  /** Records read by a load: their last use moves to now. */
  touched: { key: RecordKey; size: number }[];
  manifests: StoredManifest[];
}

export interface RecordStore {
  /** One readonly transaction; a promise per key (undefined: miss, timeout or failure). */
  read(
    keys: RecordKey[],
    signal: AbortSignal,
  ): { records: Promise<StoredRecord | undefined>[]; done: Promise<number> };
  manifest(key: ManifestKey, signal: AbortSignal): Promise<StoredManifest | undefined>;
  /**
   * Evicts, then stores the batch; resolves false when nothing was committed. The first
   * committed write of a store also deletes every row of other builds.
   */
  write(batch: WriteBatch): Promise<boolean>;
  remove(key: RecordKey): Promise<void>;
}

export const recordKey = (r: RecordKey | StoredRecord): RecordKey =>
  Array.isArray(r) ? r : [r.build, r.projectId, r.digest, r.width, r.height, r.theme];

/**
 * Least recently used rows to delete so that the store, with `incoming` added (a key
 * already stored is replaced, not added), holds at most `entries` rows and `bytes` bytes.
 */
export function evictions(
  rows: Usage[],
  incoming: { key: RecordKey; size: number }[],
  bounds = { entries: RECORD_ENTRIES, bytes: RECORD_BYTES },
): RecordKey[] {
  const replaced = new Set(incoming.map((item) => JSON.stringify(item.key)));
  const kept = rows
    .filter((row) => !replaced.has(JSON.stringify(row.key)))
    .sort((a, b) => a.used - b.used);
  let count = kept.length + incoming.length;
  let bytes = [...kept, ...incoming].reduce((sum, item) => sum + item.size, 0);
  const evicted: RecordKey[] = [];
  for (const row of kept) {
    if (count <= bounds.entries && bytes <= bounds.bytes) break;
    evicted.push(row.key);
    count--;
    bytes -= row.size;
  }
  return evicted;
}

const valid = (value: unknown): value is StoredRecord => {
  const r = value as StoredRecord | undefined;
  return (
    !!r &&
    r.bytes instanceof ArrayBuffer &&
    typeof r.frameId === "string" &&
    Array.isArray(r.fonts) &&
    r.fonts.every((font) => typeof font?.id === "string" && typeof font?.hash === "string")
  );
};

/** A manifest whose digests are a plain object of strings; anything else reads as a miss. */
export const validManifest = (value: unknown): value is StoredManifest => {
  const digests = (value as StoredManifest | undefined)?.digests;
  return (
    typeof digests === "object" &&
    digests !== null &&
    !Array.isArray(digests) &&
    Object.values(digests).every((digest) => typeof digest === "string")
  );
};

/** Keys of `build` sort between `[build]` and `[build, []]` (an array sorts after strings). */
const own = (build: string) => IDBKeyRange.bound([build], [build, []]);
const others = (build: string) => [
  IDBKeyRange.upperBound([build], true),
  IDBKeyRange.lowerBound([build, []], true),
];

export function indexedDbStore(
  build: string,
  bounds = { entries: RECORD_ENTRIES, bytes: RECORD_BYTES },
): RecordStore | undefined {
  if (typeof indexedDB === "undefined") return undefined;
  let opening: Promise<IDBDatabase | undefined> | undefined;
  /** Other builds' rows were deleted by a committed write of this store (once per worker). */
  let swept = false;
  const database = () =>
    (opening ??= new Promise((resolve) => {
      let done = false;
      const finish = (db?: IDBDatabase) => {
        if (done) return void db?.close();
        done = true;
        clearTimeout(timer);
        if (!db) opening = undefined;
        resolve(db);
      };
      const timer = setTimeout(() => finish(), OPEN_DEADLINE);
      try {
        const request = indexedDB.open(NAME, 1);
        request.onupgradeneeded = () => {
          const db = request.result;
          const keyPath = ["build", "projectId", "digest", "width", "height", "theme"];
          db.createObjectStore("records", { keyPath });
          db.createObjectStore("usage", { keyPath });
          db.createObjectStore("manifests", {
            keyPath: ["build", "projectId", "etag", "framesKey"],
          }).createIndex("used", "used");
        };
        request.onsuccess = () => {
          const db = request.result;
          db.onversionchange = () => {
            db.close();
            opening = undefined;
          };
          finish(db);
        };
        request.onerror = request.onblocked = () => finish();
      } catch {
        finish();
      }
    }));

  /** Runs `body` in one transaction; resolves whether it committed before `deadline`. */
  async function transaction(
    names: string[],
    mode: IDBTransactionMode,
    deadline: number,
    signal: AbortSignal | undefined,
    body: (tx: IDBTransaction) => void,
  ): Promise<boolean> {
    if (signal?.aborted) return false;
    const db = await database();
    if (!db || signal?.aborted) return false;
    return new Promise((resolve) => {
      let tx: IDBTransaction | undefined;
      const abort = () => {
        try {
          tx?.abort();
        } catch {
          /* Already settled. */
        }
      };
      const timer = setTimeout(() => {
        abort();
        finish(false);
      }, deadline);
      const finish = (ok: boolean) => {
        clearTimeout(timer);
        signal?.removeEventListener("abort", abort);
        resolve(ok);
      };
      try {
        tx = db.transaction(names, mode);
        tx.oncomplete = () => finish(true);
        tx.onerror = tx.onabort = () => finish(false);
        signal?.addEventListener("abort", abort, { once: true });
        body(tx);
      } catch {
        abort();
        finish(false);
      }
    });
  }

  return {
    read(keys, signal) {
      const resolvers: ((value?: StoredRecord) => void)[] = [];
      const records = keys.map(
        () => new Promise<StoredRecord | undefined>((resolve) => resolvers.push(resolve)),
      );
      const settle = () => resolvers.forEach((resolve) => resolve(undefined));
      const start = performance.now();
      const done = transaction(["records"], "readonly", READ_DEADLINE, signal, (tx) => {
        const store = tx.objectStore("records");
        keys.forEach((key, i) => {
          const request = store.get(key);
          request.onsuccess = () => resolvers[i](valid(request.result) ? request.result : undefined);
        });
      }).then(() => {
        settle();
        return performance.now() - start;
      });
      return { records, done };
    },
    async manifest(key, signal) {
      let value: StoredManifest | undefined;
      await transaction(["manifests"], "readonly", OPEN_DEADLINE, signal, (tx) => {
        const request = tx.objectStore("manifests").get(key);
        request.onsuccess = () => (value = request.result);
      });
      return validManifest(value) ? value : undefined;
    },
    async write(batch) {
      const used = Date.now();
      const records = batch.records.filter((r) => r.size <= RECORD_ENTRY_BYTES);
      const sweep = !swept;
      const ok = await transaction(["records", "usage", "manifests"], "readwrite", WRITE_DEADLINE, undefined, (tx) => {
        const [store, usage, manifests] = ["records", "usage", "manifests"].map((n) => tx.objectStore(n));
        // Records of other engine builds are never read by this one. They go once per worker
        // (its first write), not on every write: a tab still open on another build (kept across
        // a deploy) would otherwise wipe this build's rows on each of its loads, and back.
        if (sweep)
          for (const range of others(build))
            for (const s of [store, usage, manifests]) s.delete(range);
        const rows = usage.getAll(own(build));
        rows.onsuccess = () => {
          // Records this batch's loads read are as recent as the ones it adds: their last use
          // moves to now before ranking, so a load never evicts what it has just imported.
          const touched = new Set(batch.touched.map(({ key }) => JSON.stringify(key)));
          const current = (rows.result as Usage[]).map((row) =>
            touched.has(JSON.stringify(row.key)) ? { ...row, used } : row,
          );
          const incoming = records.map((r) => ({ key: recordKey(r), size: r.size }));
          const evicted = new Set<string>();
          for (const key of evictions(current, incoming, bounds)) {
            evicted.add(JSON.stringify(key));
            store.delete(key);
            usage.delete(key);
          }
          const replaced = new Set(incoming.map(({ key }) => JSON.stringify(key)));
          for (const record of records) {
            store.put(record);
            usage.put({ ...rowKey(record), key: recordKey(record), size: record.size, used });
          }
          // Only rows that still have their record (stored, not evicted, not just replaced).
          for (const row of current) {
            const id = JSON.stringify(row.key);
            if (touched.has(id) && !evicted.has(id) && !replaced.has(id))
              usage.put({ ...rowKey(row.key), key: row.key, size: row.size, used });
          }
        };
        for (const manifest of batch.manifests) manifests.put({ ...manifest, used });
        if (!batch.manifests.length) return;
        const count = manifests.count(own(build));
        count.onsuccess = () => {
          let excess = count.result - MANIFESTS;
          if (excess <= 0) return;
          const cursor = manifests.index("used").openCursor();
          cursor.onsuccess = () => {
            const item = cursor.result;
            if (!item || excess <= 0) return;
            if ((item.value as StoredManifest).build === build) {
              item.delete();
              excess--;
            }
            item.continue();
          };
        };
      });
      if (ok && sweep) swept = true;
      return ok;
    },
    async remove(key) {
      await transaction(["records", "usage"], "readwrite", OPEN_DEADLINE, undefined, (tx) => {
        tx.objectStore("records").delete(key);
        tx.objectStore("usage").delete(key);
      });
    },
  };
}

/** The key fields of a usage row (its object store shares the records' key path). */
function rowKey(record: StoredRecord | RecordKey) {
  const [build, projectId, digest, width, height, theme] = recordKey(record);
  return { build, projectId, digest, width, height, theme };
}
