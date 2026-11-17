import { useMemo } from "react";
import { fitFrameBoard, type FrameSize } from "../scene/frameLayout";
import type { SceneView } from "./useScene";

/** Scene publication changes paint inputs, but only frame geometry changes board packing. */
export function useCanvasFrames(view: SceneView, frameName: string) {
  const { scene, resources } = view;
  const frames = useMemo(
    () => view.frames ?? (scene && resources
      ? [{
          id: scene.provenance.frameId,
          name: frameName,
          ...scene.provenance.viewport,
          scene,
          resources,
          revision: scene.provenance.revision,
        }]
      : []),
    [view.frames, scene, resources, frameName],
  );
  const geometry = useMemo(
    () => JSON.stringify(frames.map(({ id, width, height }) => ({ id, width, height }))),
    [frames],
  );
  // Decode our own serialization so the layout depends only on authored geometry, never
  // on progressive scene/resource objects, their revision or the pane size.
  const boardLayout = useMemo(() => fitFrameBoard(JSON.parse(geometry) as FrameSize[]), [geometry]);
  const all = Boolean(view.frames);
  const content = all
    ? frames.length ? boardLayout : undefined
    : scene?.provenance.viewport;
  return { frames, boardLayout, content };
}
