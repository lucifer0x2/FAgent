import {
  indexedDbStore,
  recordKey,
  THEME,
  type RecordKey,
  type RecordStore,
  type Resource,
  type StoredManifest,
  type StoredRecord,
} from "./recordStore.ts";

/** A frame computed by a load, written once the load has drawn it (`flush`). */
export interface Computed {
  projectId: string;
  frameId: string;
  digest: string;
  width: number;
  height: number;
  assets: Resource[];
}

export interface Wanted {
  frameId: string;
  digest: string;
  width: number;
  height: number;
}

/** Counters of this Worker's cache use (`status().cache`, benchmarks). */
export interface CacheStats {
  enabled: boolean;
  manifestHits: number;
  manifestMisses: number;
  lookups: number;
  hits: number;
  misses: number;
  bytesRead: number;
  /** Duration of the last batched read transaction, ms. */
  readMs: number;
  imported: number;
  rejected: number;
  written: number;
  bytesWritten: number;
  exportMs: number;
  writeFailures: number;
  pendingWrites: number;
  writing: boolean;
}

export interface SceneCacheOptions {
  /** Engine module content hash + record format; undefined (dev module) turns the cache off. */
  build(): string | undefined;
  open?(build: string): RecordStore | undefined;
  /** Hex digest of the request's frame list (manifest key). */
  hash?(text: string): Promise<string>;
  now?(): number;
}

const MAX_BATCH_RECORDS = 64;
const MAX_BATCH_BYTES = 32 * 1024 * 1024;

async function sha256(text: string) {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(bytes), (b) => b.toString(16).padStart(2, "0")).join("");
}

const exactBuffer = (bytes: Uint8Array): ArrayBuffer =>
  bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
    ? (bytes.buffer as ArrayBuffer)
    : (bytes.slice().buffer as ArrayBuffer);

/**
 * The engine Worker's persistent scene cache (Task 42): frame records by content digest,
 * and frame digests by strong snapshot ETag. It never decides access: the loader asks it
 * only for a sync that carries a snapshot the server answered in this session, and it never
 * fails a frame (anything unreadable is a miss and the frame computes).
 */
export class SceneCache {
  private opened?: { build: string; store?: RecordStore };
  private pending = new Map<string, Computed>();
  private manifests: StoredManifest[] = [];
  private touched = new Map<string, { key: RecordKey; size: number }>();
  private writing = false;
  private counters = {
    manifestHits: 0,
    manifestMisses: 0,
    lookups: 0,
    hits: 0,
    misses: 0,
    bytesRead: 0,
    readMs: 0,
    imported: 0,
    rejected: 0,
    written: 0,
    bytesWritten: 0,
    exportMs: 0,
    writeFailures: 0,
  };
  private options: SceneCacheOptions;

  constructor(options: SceneCacheOptions) {
    this.options = options;
  }

  private store() {
    const build = this.options.build();
    if (!build) return undefined;
    if (this.opened?.build !== build)
      this.opened = { build, store: (this.options.open ?? indexedDbStore)(build) };
    return this.opened.store ? { build, store: this.opened.store } : undefined;
  }

  private now() {
    return this.options.now?.() ?? performance.now();
  }

  /** Manifest key of a frame list (undefined when the cache is off or cannot hash). */
  async framesKey(frames: { frameId: string; width: number; height: number }[]) {
    if (!this.store()) return undefined;
    try {
      return await (this.options.hash ?? sha256)(JSON.stringify([THEME, frames]));
    } catch {
      return undefined;
    }
  }

  /** Digests stored for this snapshot representation (`etag`) and frame list, if any. */
  async digests(
    projectId: string,
    etag: string,
    framesKey: string,
    signal: AbortSignal,
  ): Promise<Map<string, string> | undefined> {
    const opened = this.store();
    if (!opened) return undefined;
    const manifest = await opened.store
      .manifest([opened.build, projectId, etag, framesKey], signal)
      .catch(() => undefined);
    if (!manifest) {
      this.counters.manifestMisses++;
      return undefined;
    }
    this.counters.manifestHits++;
    return new Map(Object.entries(manifest.digests));
  }

  /** Queues the digests `frame_keys` returned for this ETag and frame list. */
  rememberDigests(projectId: string, etag: string, framesKey: string, digests: Map<string, string>) {
    const build = this.options.build();
    if (!build || !digests.size) return;
    this.manifests.push({
      build,
      projectId,
      etag,
      framesKey,
      digests: Object.fromEntries(digests),
      used: 0,
    });
  }

  /**
   * One readonly transaction for every wanted frame; each frame's record arrives on its own
   * (undefined: miss). Aborting `signal` (a newer sync) ends the read.
   */
  lookup(projectId: string, wanted: Wanted[], signal: AbortSignal) {
    const found = new Map<string, Promise<StoredRecord | undefined>>();
    const opened = this.store();
    if (!opened || !wanted.length) return found;
    const keys = wanted.map(
      (w): RecordKey => [opened.build, projectId, w.digest, w.width, w.height, THEME],
    );
    this.counters.lookups += keys.length;
    const { records, done } = opened.store.read(keys, signal);
    wanted.forEach((w, i) =>
      found.set(
        w.frameId,
        records[i].then((record) => {
          if (record) {
            this.counters.hits++;
            this.counters.bytesRead += record.bytes.byteLength;
          } else this.counters.misses++;
          return record;
        }),
      ),
    );
    void done.then((ms) => (this.counters.readMs = Math.round(ms)));
    return found;
  }

  /** An imported record: its last use moves to now at the next write. */
  imported(record: StoredRecord) {
    this.counters.imported++;
    const key = recordKey(record);
    this.touched.set(JSON.stringify(key), { key, size: record.size });
  }

  /** A record the engine refused (corrupt, another format): removed; the frame computes. */
  rejected(record: StoredRecord) {
    this.counters.rejected++;
    void this.store()?.store.remove(recordKey(record));
  }

  /** A frame this load computed; written by `flush` after the load. */
  computed(frame: Computed) {
    if (!this.options.build()) return;
    this.pending.set(JSON.stringify([frame.projectId, frame.digest, frame.width, frame.height]), frame);
  }

  /**
   * Writes what loads computed, in batches off the critical path: `pace` runs before each
   * record export (it yields to page messages and waits while a load runs), and
   * `exportRecord` returns the shown frame's record, or undefined once it is gone.
   */
  async flush(
    exportRecord: (frame: Computed) => { bytes: Uint8Array; fonts: Resource[] } | undefined,
    pace: () => Promise<void>,
  ) {
    if (this.writing) return;
    this.writing = true;
    try {
      while (this.pending.size || this.manifests.length || this.touched.size) {
        const opened = this.store();
        if (!opened) {
          this.pending.clear();
          this.manifests = [];
          this.touched.clear();
          break;
        }
        const records: StoredRecord[] = [];
        let bytes = 0;
        for (const [key, frame] of this.pending) {
          await pace();
          this.pending.delete(key);
          const start = this.now();
          const exported = exportRecord(frame);
          this.counters.exportMs += this.now() - start;
          if (!exported) continue;
          const size = exported.bytes.byteLength;
          records.push({
            build: opened.build,
            projectId: frame.projectId,
            digest: frame.digest,
            width: frame.width,
            height: frame.height,
            theme: THEME,
            frameId: frame.frameId,
            bytes: exactBuffer(exported.bytes),
            fonts: exported.fonts,
            assets: frame.assets.map(({ id, hash }) => ({ id, hash })),
            size,
          });
          bytes += size;
          if (records.length >= MAX_BATCH_RECORDS || bytes >= MAX_BATCH_BYTES) break;
        }
        const batch = {
          records,
          touched: [...this.touched.values()],
          manifests: this.manifests.splice(0),
        };
        this.touched.clear();
        await pace();
        if (await opened.store.write(batch)) {
          this.counters.written += records.length;
          this.counters.bytesWritten += bytes;
        } else this.counters.writeFailures++;
      }
    } finally {
      this.writing = false;
    }
  }

  stats(): CacheStats {
    return {
      enabled: Boolean(this.store()),
      ...this.counters,
      exportMs: Math.round(this.counters.exportMs),
      pendingWrites: this.pending.size + this.manifests.length + this.touched.size,
      writing: this.writing,
    };
  }
}
