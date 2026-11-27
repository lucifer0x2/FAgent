import type { EngineApi } from "./engineApi.ts";
import type {
  EngineSummary,
  FrameRequest,
  PageMessage,
  Placement,
  WorkerMessage,
} from "./protocol.ts";
import type { StoredRecord } from "./recordStore.ts";
import type { SceneCache } from "./sceneCache.ts";
import { snapshotResources } from "../scene/manifests.ts";
import type { Snapshot } from "../types.ts";

type Sync = Extract<PageMessage, { type: "sync" }>;
type Manifest = { id: string; hash: string; url: string; mediaType?: string };
type Manifests = ReturnType<typeof snapshotResources>;
type Shown = {
  frameId: string;
  revision: number;
  summary: EngineSummary;
  missing: string[];
  restored?: boolean;
};

export interface FrameLoaderDeps {
  engine: () => EngineApi;
  /** Validated immutable bytes (SHA-256 against the manifest). */
  fetchBytes(manifest: Manifest): Promise<ArrayBuffer>;
  /** Validated font bytes (a persistent cache in front of `fetchBytes`); default `fetchBytes`. */
  fetchFont?(manifest: Manifest): Promise<ArrayBuffer>;
  /** Straight RGBA8 pixels of an encoded image. */
  decodeImage(
    bytes: ArrayBuffer,
    mediaType: string,
  ): Promise<{ data: Uint8Array; width: number; height: number }>;
  post(message: WorkerMessage): void;
  requestRender(): void;
  /** Lets camera messages and renders run between frames. */
  yieldTask(): Promise<void>;
  /**
   * Persistent scene cache (Task 42). Without it every frame computes, exactly as before;
   * with it, frames whose records are stored are imported instead.
   */
  cache?: SceneCache;
}

const message = (cause: unknown, fallback: string) =>
  typeof (cause as { message?: unknown })?.message === "string"
    ? (cause as { message: string }).message
    : fallback;

/** A newer sync took over while a frame loaded. */
const SUPERSEDED = Symbol("superseded");

/**
 * Computes the requested frames one at a time, in the engine, from the accepted snapshot.
 * A newer `sync` supersedes the running one between steps; frames already shown stay on
 * screen (marked by their older revision) until their replacement is committed.
 *
 * With a scene cache: a strong snapshot ETag seen before gives the frame digests without
 * `frame_keys`; frames the engine does not show are read from disk in one batch (started
 * before the first frame, so it overlaps the backend attach) and imported instead of
 * computed; computed frames are written after the load has drawn them.
 */
export class FrameLoader {
  private generation = 0;
  private accepted?: { projectId: string; revision: number; snapshot: Snapshot };
  private failedFonts = new Set<string>();
  /**
   * What the page was told about each committed frame. A reused frame resends it: the page
   * may never have received or kept the first answer (another scope showed the same frame,
   * or a newer sync replaced the view before it published), and without a summary it
   * would keep that frame's skeleton although the engine draws it.
   */
  private shown = new Map<string, { summary: EngineSummary; missing: string[] }>();
  /** Loads in progress, and their end (cache writes wait for it). */
  private loading = 0;
  private idle: Promise<void> = Promise.resolve();
  private settle = () => {};
  /** The running sync's cache reads; a newer sync aborts them. */
  private reads?: AbortController;

  private deps: FrameLoaderDeps;

  constructor(deps: FrameLoaderDeps) {
    this.deps = deps;
  }

  /**
   * Places the board's frames. The engine releases every frame the layout leaves out
   * (`set_layout`), so what the page was told about those frames is dropped too: `shown`
   * holds only placed frames instead of every frame computed since the project opened.
   */
  setLayout(placements: Placement[]) {
    this.deps.engine().set_layout(JSON.stringify(placements));
    const placed = new Set(placements.map((placement) => placement.id));
    for (const frameId of this.shown.keys())
      if (!placed.has(frameId)) this.shown.delete(frameId);
  }

  async sync(request: Sync) {
    this.generation = request.generation;
    this.reads?.abort();
    const reads = (this.reads = new AbortController());
    const { engine, post } = { engine: this.deps.engine(), post: this.deps.post };
    const current = () => this.generation === request.generation;
    const cache = this.deps.cache;
    const end = this.begin();
    try {
      if (request.snapshot) {
        if (
          request.snapshot.projectId !== request.projectId ||
          request.snapshot.revision !== request.revision
        )
          throw new Error("Engine snapshot does not match its request.");
        engine.load_snapshot(JSON.stringify(request.snapshot));
        // Another project's frames are released by the engine (`load_snapshot`).
        if (this.accepted && this.accepted.projectId !== request.projectId) this.shown.clear();
        this.accepted = {
          projectId: request.projectId,
          revision: request.revision,
          snapshot: request.snapshot,
        };
      }
      const accepted = this.accepted;
      if (
        accepted?.projectId !== request.projectId ||
        accepted.revision !== request.revision
      )
        throw new Error("The engine is missing this snapshot revision.");
      if (request.bypass) this.failedFonts.clear();
      // The cache is read only from here on: this sync carries a snapshot the server answered.
      const digests = cache
        ? await this.cachedKeys(cache, engine, request, reads.signal)
        : this.keys(engine, request);
      if (!current()) return;
      const records =
        cache && !request.bypass
          ? this.lookup(cache, engine, request, digests, reads.signal)
          : undefined;
      for (const frame of request.frames) {
        await this.deps.yieldTask();
        if (!current()) return;
        const digest = digests.get(frame.frameId) ?? "";
        if (
          !request.bypass &&
          engine.reuse(frame.frameId, digest, request.revision)
        ) {
          const shown = this.shown.get(frame.frameId);
          if (shown) shown.summary = { ...shown.summary, revision: request.revision };
          post({
            type: "frame",
            generation: request.generation,
            frameId: frame.frameId,
            revision: request.revision,
            reused: true,
            ...(shown ? { summary: shown.summary, missing: shown.missing } : {}),
          });
          continue;
        }
        try {
          const manifests = snapshotResources(accepted.snapshot, {
            projectId: request.projectId,
            revision: request.revision,
            ...frame,
          });
          const stored = records ? await records.get(frame.frameId) : undefined;
          if (!current()) return;
          let result: Shown | typeof SUPERSEDED | undefined = stored
            ? await this.restore(engine, manifests, request, frame, digest, stored, current)
            : undefined;
          if (result === SUPERSEDED) return;
          if (!result) {
            result = await this.load(engine, manifests, request, frame, digest, current);
            if (!result) return;
            if (cache && digest)
              cache.computed({
                projectId: request.projectId,
                frameId: frame.frameId,
                digest,
                width: frame.width,
                height: frame.height,
                assets: result.summary.assets,
              });
          }
          post({ type: "frame", generation: request.generation, ...result });
        } catch (cause) {
          engine.discard(frame.frameId);
          if (!current()) return;
          post({
            type: "frame",
            generation: request.generation,
            frameId: frame.frameId,
            revision: request.revision,
            error: message(cause, "Frame unavailable."),
          });
        }
      }
      if (current()) post({ type: "synced", generation: request.generation });
    } catch (cause) {
      if (current())
        post({
          type: "synced",
          generation: request.generation,
          error: message(cause, "Engine scene computation failed."),
        });
    } finally {
      end();
      // Off the critical path: after the draw that showed this load's frames.
      if (cache) void this.persist(cache);
    }
  }

  /** Marks a load as running until the returned function is called. */
  private begin() {
    if (this.loading++ === 0) this.idle = new Promise((resolve) => (this.settle = resolve));
    let ended = false;
    return () => {
      if (ended) return;
      ended = true;
      if (--this.loading === 0) this.settle();
    };
  }

  private keys(engine: EngineApi, request: Sync) {
    const digests = new Map<string, string>();
    try {
      const keys: { frameId: string; digest?: string }[] = JSON.parse(
        engine.frame_keys(request.revision, JSON.stringify(request.frames)),
      );
      for (const key of keys)
        if (key.digest) digests.set(key.frameId, key.digest);
    } catch {
      // Frames are then computed without reuse.
    }
    return digests;
  }

  /** Digests stored for this ETag and frame list, else `frame_keys` (then stored). */
  private async cachedKeys(cache: SceneCache, engine: EngineApi, request: Sync, signal: AbortSignal) {
    const framesKey = request.etag ? await cache.framesKey(request.frames) : undefined;
    if (request.etag && framesKey && !request.bypass) {
      const known = await cache.digests(request.projectId, request.etag, framesKey, signal);
      if (known) return known;
    }
    const digests = this.keys(engine, request);
    if (request.etag && framesKey)
      cache.rememberDigests(request.projectId, request.etag, framesKey, digests);
    return digests;
  }

  /** One batched read of the frames the engine does not show with their current digest. */
  private lookup(
    cache: SceneCache,
    engine: EngineApi,
    request: Sync,
    digests: Map<string, string>,
    signal: AbortSignal,
  ) {
    const wanted = request.frames.flatMap((frame) => {
      const digest = digests.get(frame.frameId);
      return digest && !engine.shows(frame.frameId, digest)
        ? [{ frameId: frame.frameId, digest, width: frame.width, height: frame.height }]
        : [];
    });
    return cache.lookup(request.projectId, wanted, signal);
  }

  /** Fetches and loads the fonts in `required` the engine lacks; false: none fetched. */
  private async fonts(
    engine: EngineApi,
    manifests: Manifests,
    required: { id: string; hash: string }[],
    current: () => boolean,
  ): Promise<boolean | typeof SUPERSEDED> {
    let fetchedFont = false;
    for (const font of required) {
      if (engine.has_font(font.id, font.hash)) continue;
      const manifest = manifests.fonts.find(
        (item) => item.id === font.id && item.hash === font.hash,
      );
      const key = `${font.id}:${font.hash}`;
      if (!manifest || this.failedFonts.has(key))
        throw new Error(`Missing font: ${font.id}`);
      let bytes: ArrayBuffer;
      try {
        bytes = await (this.deps.fetchFont ?? this.deps.fetchBytes)(manifest);
      } catch (cause) {
        this.failedFonts.add(key);
        throw new Error(`Missing font: ${font.id} (${message(cause, "unavailable")})`);
      }
      if (!current()) return SUPERSEDED;
      engine.load_font(font.id, font.hash, new Uint8Array(bytes));
      fetchedFont = true;
    }
    // Page messages ran while the font was fetched: a camera or layout received then is drawn
    // before this frame computes, as it would be between two frames.
    if (fetchedFont) {
      await this.deps.yieldTask();
      if (!current()) return SUPERSEDED;
    }
    return fetchedFont;
  }

  private async load(
    engine: EngineApi,
    manifests: Manifests,
    request: Sync,
    frame: FrameRequest,
    digest: string,
    current: () => boolean,
  ) {
    const required: { id: string; hash: string }[] = JSON.parse(
      engine.required_fonts(
        request.revision,
        frame.frameId,
        frame.width,
        frame.height,
      ),
    );
    if ((await this.fonts(engine, manifests, required, current)) === SUPERSEDED) return;
    const summary = JSON.parse(
      engine.compute(
        request.revision,
        frame.frameId,
        frame.width,
        frame.height,
        digest,
      ),
    );
    return this.show(engine, manifests, request, frame, summary, current);
  }

  /**
   * A stored record staged as `compute` would stage it, after its fonts are loaded. A record
   * the engine refuses is removed and the frame computes (undefined).
   */
  private async restore(
    engine: EngineApi,
    manifests: Manifests,
    request: Sync,
    frame: FrameRequest,
    digest: string,
    stored: StoredRecord,
    current: () => boolean,
  ): Promise<Shown | typeof SUPERSEDED | undefined> {
    try {
      if ((await this.fonts(engine, manifests, stored.fonts, current)) === SUPERSEDED)
        return SUPERSEDED;
    } catch {
      // The stored font list is not the record (a damaged entry, or a font the network
      // cannot deliver): computing asks the engine which fonts the frame needs and reports
      // a font that stays unavailable, as without a cache.
      return current() ? undefined : SUPERSEDED;
    }
    let summary: EngineSummary;
    try {
      summary = JSON.parse(
        engine.import_record(
          new Uint8Array(stored.bytes),
          request.revision,
          frame.frameId,
          frame.width,
          frame.height,
          digest,
        ),
      );
    } catch {
      this.deps.cache?.rejected(stored);
      return undefined;
    }
    this.deps.cache?.imported(stored);
    const shown = await this.show(engine, manifests, request, frame, summary, current);
    return shown ? { ...shown, restored: true } : SUPERSEDED;
  }

  /** Images of a staged frame (missing ones reported), then the commit that shows it. */
  private async show(
    engine: EngineApi,
    manifests: Manifests,
    request: Sync,
    frame: FrameRequest,
    summary: EngineSummary,
    current: () => boolean,
  ): Promise<Shown | undefined> {
    const missing: string[] = [];
    for (const asset of summary.assets) {
      if (engine.has_image(asset.id, asset.hash)) continue;
      const manifest = manifests.assets.find(
        (item) => item.id === asset.id && item.hash === asset.hash,
      );
      try {
        if (!manifest) throw new Error("not in snapshot");
        const bytes = await this.deps.fetchBytes(manifest);
        const image = await this.deps.decodeImage(bytes, manifest.mediaType);
        if (!current()) return;
        engine.add_image(asset.id, asset.hash, image.data, image.width, image.height);
      } catch {
        if (!current()) return;
        missing.push(`Missing asset: ${asset.id}`);
      }
    }
    if (!current()) {
      engine.discard(frame.frameId);
      return;
    }
    // The staged scene is gone only if a layout released the frame meanwhile; never report
    // a frame as shown that the engine does not draw.
    if (!engine.commit(frame.frameId))
      throw new Error("Frame was released before it could be shown.");
    this.shown.set(frame.frameId, { summary, missing });
    this.deps.requestRender();
    return {
      frameId: frame.frameId,
      revision: request.revision,
      summary,
      missing,
    };
  }

  /** Writes the records of computed frames, one export per yield, never during a load. */
  private persist(cache: SceneCache) {
    return cache.flush(
      (frame) => {
        try {
          const engine = this.deps.engine();
          const bytes = engine.export_record(frame.frameId, frame.digest);
          return { bytes, fonts: JSON.parse(engine.record_fonts(frame.frameId)) };
        } catch {
          // Released or replaced since: its record is no longer the shown content.
          return undefined;
        }
      },
      async () => {
        await this.deps.yieldTask();
        while (this.loading) await this.idle;
      },
    );
  }
}
