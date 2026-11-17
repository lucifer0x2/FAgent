import type { EngineApi } from "./engineApi.ts";
import type {
  EngineSummary,
  FrameRequest,
  PageMessage,
  Placement,
  WorkerMessage,
} from "./protocol.ts";
import { snapshotResources } from "../scene/manifests.ts";
import type { Snapshot } from "../types.ts";

type Sync = Extract<PageMessage, { type: "sync" }>;
type Manifest = { id: string; hash: string; url: string; mediaType?: string };

export interface FrameLoaderDeps {
  engine: () => EngineApi;
  /** Validated immutable bytes (SHA-256 against the manifest). */
  fetchBytes(manifest: Manifest): Promise<ArrayBuffer>;
  /** Straight RGBA8 pixels of an encoded image. */
  decodeImage(
    bytes: ArrayBuffer,
    mediaType: string,
  ): Promise<{ data: Uint8Array; width: number; height: number }>;
  post(message: WorkerMessage): void;
  requestRender(): void;
  /** Lets camera messages and renders run between frames. */
  yieldTask(): Promise<void>;
}

const message = (cause: unknown, fallback: string) =>
  typeof (cause as { message?: unknown })?.message === "string"
    ? (cause as { message: string }).message
    : fallback;

/**
 * Computes the requested frames one at a time, in the engine, from the accepted snapshot.
 * A newer `sync` supersedes the running one between steps; frames already shown stay on
 * screen (marked by their older revision) until their replacement is committed.
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
    const { engine, post } = { engine: this.deps.engine(), post: this.deps.post };
    const current = () => this.generation === request.generation;
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
      const digests = this.keys(engine, request);
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
          const result = await this.load(engine, accepted.snapshot, request, frame, digest, current);
          if (!result) return;
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
    }
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

  private async load(
    engine: EngineApi,
    snapshot: Snapshot,
    request: Sync,
    frame: FrameRequest,
    digest: string,
    current: () => boolean,
  ) {
    const manifests = snapshotResources(snapshot, {
      projectId: request.projectId,
      revision: request.revision,
      ...frame,
    });
    const required: { id: string; hash: string }[] = JSON.parse(
      engine.required_fonts(
        request.revision,
        frame.frameId,
        frame.width,
        frame.height,
      ),
    );
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
        bytes = await this.deps.fetchBytes(manifest);
      } catch (cause) {
        this.failedFonts.add(key);
        throw new Error(`Missing font: ${font.id} (${message(cause, "unavailable")})`);
      }
      if (!current()) return;
      engine.load_font(font.id, font.hash, new Uint8Array(bytes));
    }
    const summary = JSON.parse(
      engine.compute(
        request.revision,
        frame.frameId,
        frame.width,
        frame.height,
        digest,
      ),
    );
    const missing: string[] = [];
    for (const asset of summary.assets as { id: string; hash: string }[]) {
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
}
