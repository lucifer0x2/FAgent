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
/** Oldest entries go first above this many fonts. */
export const FONT_ENTRIES = 64;

type Manifest = { id: string; hash: string; url: string; mediaType?: string };

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
}

/** `fetchChecked` with a persistent cache in front of it. */
export function cachedFontBytes(
  fetchChecked: (manifest: Manifest) => Promise<ArrayBuffer>,
  options: FontCacheOptions,
) {
  const key = (hash: string) => new URL(`/__fagent/engine-fonts/${hash}`, options.origin).href;
  let opened: Promise<Cache | undefined> | undefined;
  const open = () =>
    (opened ??= (options.storage?.open(NAME) ?? Promise.resolve(undefined)).catch(() => undefined));
  const store = async (hash: string, bytes: ArrayBuffer) => {
    const cache = await open();
    if (!cache) return;
    await cache.put(key(hash), new Response(bytes.slice(0)));
    const keys = await cache.keys();
    for (const old of keys.slice(0, Math.max(0, keys.length - FONT_ENTRIES)))
      await cache.delete(old);
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
