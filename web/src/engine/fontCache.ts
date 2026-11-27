/**
 * Verified font bytes by content hash in the engine Worker's Cache API (Task 42 item 5).
 * The server sends font bytes without caching headers, so every load fetched and
 * hash-checked each font again before the first frame could be imported.
 *
 * A hit is hash-checked again before use (a damaged entry is deleted and fetched); only
 * bytes that passed the fetch path's SHA-256 check are stored. Any Cache API failure falls
 * back to the network. Keys are synthetic same-origin URLs naming only the hash; the cache
 * holds no credentials, tokens or project data beyond the font bytes themselves.
 */
const NAME = "fagent.engine-fonts";
/** Oldest entries go first above this many fonts or this many bytes. */
export const FONT_ENTRIES = 64;
/** About six large CJK fonts (~20 MB each), or the 64 entries of typical Latin fonts. */
export const FONT_BYTES = 128 * 1024 * 1024;
/** Byte length of a stored entry, so eviction never reads font bodies. */
const SIZE_HEADER = "x-fagent-bytes";

type Manifest = { id: string; hash: string; url: string; mediaType?: string };

/** Stored size of `response`; an entry without the header is measured once from its body. */
async function entrySize(response: Response | undefined) {
  if (!response) return 0;
  const size = Number(response.headers.get(SIZE_HEADER));
  return response.headers.has(SIZE_HEADER) && Number.isFinite(size) && size >= 0
    ? size
    : (await response.blob()).size;
}

async function sha256(bytes: ArrayBuffer) {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

export interface FontCacheOptions {
  /** Whether bytes may be kept on disk (off for a module without a content hash). */
  enabled(): boolean;
  /** The Cache API (`caches`); absent in tests and insecure contexts. */
  storage?: CacheStorage;
  origin: string;
  /** Entry and byte bounds (tests); default `FONT_ENTRIES` / `FONT_BYTES`. */
  bounds?: { entries: number; bytes: number };
}

/** `fetchChecked` with a persistent cache in front of it. */
export function cachedFontBytes(
  fetchChecked: (manifest: Manifest) => Promise<ArrayBuffer>,
  options: FontCacheOptions,
) {
  const bounds = options.bounds ?? { entries: FONT_ENTRIES, bytes: FONT_BYTES };
  const key = (hash: string) => new URL(`/__fagent/engine-fonts/${hash}`, options.origin).href;
  let opened: Promise<Cache | undefined> | undefined;
  // Shared while pending or open; a failed open is forgotten, so a later load tries again
  // (this one fetches from the network).
  const open = () =>
    (opened ??= (options.storage?.open(NAME) ?? Promise.resolve(undefined)).catch(() => {
      opened = undefined;
      return undefined;
    }));
  /** Stored entries, oldest first (insertion order: a put moves its key last), with sizes. */
  const entries = async (cache: Cache) => {
    const keys = await cache.keys();
    const sizes = await Promise.all(keys.map(async (request) => entrySize(await cache.match(request))));
    return { keys, sizes };
  };
  const put = (cache: Cache, hash: string, bytes: ArrayBuffer) =>
    cache.put(
      key(hash),
      new Response(bytes.slice(0), { headers: { [SIZE_HEADER]: String(bytes.byteLength) } }),
    );
  const store = async (hash: string, bytes: ArrayBuffer) => {
    if (bytes.byteLength > bounds.bytes) return;
    const cache = await open();
    if (!cache) return;
    try {
      await put(cache, hash, bytes);
    } catch (cause) {
      if ((cause as { name?: unknown })?.name !== "QuotaExceededError") throw cause;
      // The origin's quota is full (the failed put is rolled back): free at least this font's
      // size from the oldest entries, then try once more. The live bytes are used either way.
      const { keys, sizes } = await entries(cache);
      for (let i = 0, freed = 0; i < keys.length && freed < bytes.byteLength; i++) {
        await cache.delete(keys[i]);
        freed += sizes[i];
      }
      await put(cache, hash, bytes);
    }
    // The oldest go until both bounds hold (never the font just stored, which is last).
    const { keys, sizes } = await entries(cache);
    let count = keys.length;
    let total = sizes.reduce((sum, size) => sum + size, 0);
    for (let i = 0; i < keys.length - 1 && (count > bounds.entries || total > bounds.bytes); i++) {
      await cache.delete(keys[i]);
      count--;
      total -= sizes[i];
    }
  };
  return async (manifest: Manifest): Promise<ArrayBuffer> => {
    if (!options.enabled() || !options.storage || !/^[0-9a-f]{64}$/.test(manifest.hash))
      return fetchChecked(manifest);
    try {
      const cache = await open();
      const hit = await cache?.match(key(manifest.hash));
      if (hit) {
        const bytes = await hit.arrayBuffer();
        if ((await sha256(bytes)) === manifest.hash) return bytes;
        await cache?.delete(key(manifest.hash));
      }
    } catch {
      // Unreadable cache: fetched below.
    }
    const bytes = await fetchChecked(manifest);
    void store(manifest.hash, bytes).catch(() => {});
    return bytes;
  };
}
