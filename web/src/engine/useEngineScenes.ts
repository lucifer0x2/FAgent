import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { diagnosticLabel } from "../diagnostics";
import type { FrameSceneView, SceneView } from "../hooks/useScene";
import { useSnapshotEtag } from "../hooks/useSnapshot";
import { publishScheduler } from "../scene/publishScheduler";
import { pageView, type FrameTarget } from "../scene/staleFrames";
import type { Snapshot } from "../types";
import { engineClient } from "./client";

const EMPTY_VIEW: SceneView = { status: "empty", missing: [], diagnostics: [] };

type Loaded = {
  scope: string;
  key: string;
  retryKey: number;
  entries: FrameSceneView[];
  finished: boolean;
  error?: string;
  seeded: boolean;
};

/** Stale-while-revalidate: a frame the engine already shows keeps its summary until replaced. */
function seed(targets: FrameTarget[], previous: FrameSceneView[]) {
  const byId = new Map(previous.map((frame) => [frame.id, frame]));
  return targets.map((target): FrameSceneView => {
    const old = byId.get(target.id);
    return old?.summary
      ? {
          ...target,
          height: target.hugHeight ? old.summary.height : target.height,
          summary: old.summary,
          revision: old.revision,
          diagnostics: old.diagnostics,
          missing: old.missing,
          stale: true,
        }
      : { ...target };
  });
}

function toView(loaded: Loaded, board: boolean): SceneView {
  const view = pageView(loaded.entries, {
    finished: loaded.finished,
    error: loaded.error,
    seeded: loaded.seeded,
  });
  const engine = { board, frames: loaded.entries };
  if (board) return { ...view, engine };
  const only = loaded.entries[0];
  return {
    ...view,
    frames: undefined,
    progress: undefined,
    engine,
    error: view.error && (only?.error ?? only?.updateError ?? view.error),
  };
}

/**
 * The engine counterpart of useScene/usePageScenes: sends the accepted snapshot and the
 * frame targets to the Worker, and mirrors its per-frame events (summaries, errors) into a
 * SceneView. Frame geometry, staleness and failures follow the Canvas2D board semantics.
 */
export function useEngineScenes(
  enabled: boolean,
  snapshot: Snapshot | null,
  targets: FrameTarget[],
  scope: string,
  retryKey: number,
  board: boolean,
): SceneView {
  const active = enabled && Boolean(snapshot) && targets.length > 0;
  const key = active ? JSON.stringify([scope, snapshot!.revision]) : "";
  const [loaded, setLoaded] = useState<Loaded>({
    scope: "",
    key: "",
    retryKey,
    entries: [],
    finished: false,
    seeded: false,
  });
  const latest = useRef(loaded);
  useLayoutEffect(() => {
    latest.current = loaded;
  }, [loaded]);
  // The server's strong ETag for this exact snapshot: the worker's scene cache keys frame
  // digests by it. Read through a ref so that it never starts a sync of its own.
  const snapshotEtag = useSnapshotEtag(snapshot);
  const etag = useRef(snapshotEtag);
  useLayoutEffect(() => {
    etag.current = snapshotEtag;
  }, [snapshotEtag]);
  const lastRetry = useRef(retryKey);
  useEffect(() => {
    if (!active || !snapshot) return;
    const bypass = lastRetry.current !== retryKey;
    lastRetry.current = retryKey;
    const previous = latest.current;
    const seeded =
      !bypass &&
      previous.scope === scope &&
      previous.entries.some((entry) => entry.summary);
    const entries = seeded
      ? seed(targets, previous.entries)
      : targets.map((target): FrameSceneView => ({ ...target }));
    let finished = false;
    let failure: string | undefined;
    const publisher = publishScheduler(() =>
      setLoaded({
        scope,
        key,
        retryKey,
        entries: [...entries],
        finished,
        error: failure,
        seeded,
      }),
    );
    const generation = engineClient.sync(
      snapshot,
      targets.map((target) => ({
        frameId: target.id,
        width: target.width,
        height: target.height,
      })),
      bypass,
      etag.current,
    );
    const off = engineClient.onFrame((event) => {
      if (event.generation !== generation) return;
      if (event.type === "synced") {
        finished = true;
        failure = event.error;
        publisher.flush();
        return;
      }
      const index = targets.findIndex((target) => target.id === event.frameId);
      if (index < 0) return;
      const entry = entries[index],
        target = targets[index];
      if (event.error)
        entries[index] = entry.summary
          ? { ...entry, updateError: event.error }
          : { ...target, error: event.error };
      else if (event.reused && entry.summary && !event.summary) {
        const { stale: _stale, updateError: _error, ...current } = entry;
        entries[index] = { ...current, revision: event.revision };
      } else if (event.summary)
        // Computed, or reused with the summary the engine shows (this view may not have it).
        entries[index] = {
          ...target,
          height: target.hugHeight ? event.summary.height : target.height,
          summary: event.summary,
          revision: event.revision,
          diagnostics: event.summary.diagnostics.map(diagnosticLabel),
          missing: event.missing ?? [],
        };
      publisher.schedule();
    });
    publisher.schedule();
    return () => {
      off();
      publisher.cancel();
    };
  }, [active, snapshot, targets, scope, key, retryKey]);
  const view = useMemo(() => {
    // The engine canvas stays mounted (and keeps its surface) while nothing is selected.
    if (!active)
      return enabled
        ? { ...EMPTY_VIEW, ...(board && snapshot ? { frames: [] } : {}), engine: { board, frames: [] } }
        : EMPTY_VIEW;
    if (loaded.key === key && loaded.retryKey === retryKey)
      return toView(loaded, board);
    // Before the effect's first publish: keep this scope's frames, else placeholders.
    const keep =
      loaded.scope === scope &&
      loaded.retryKey === retryKey &&
      loaded.entries.some((entry) => entry.summary);
    return toView(
      {
        scope,
        key,
        retryKey,
        entries: keep
          ? seed(targets, loaded.entries)
          : targets.map((target) => ({ ...target })),
        finished: false,
        seeded: keep,
      },
      board,
    );
  }, [active, enabled, snapshot, loaded, key, scope, retryKey, targets, board]);
  return useMemo(
    () => ({ ...view, cameraKey: scope, requestedRevision: snapshot?.revision }),
    [view, scope, snapshot?.revision],
  );
}
