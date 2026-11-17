import { useCallback, useMemo, useState, type CSSProperties } from "react";
import { CanvasView } from "../components/CanvasView";
import { Inspector } from "../components/Inspector";
import { Layers } from "../components/Layers";
import { NodeActionsProvider } from "../components/NodeActionsProvider";
import { PanelResizer } from "../components/PanelResizer";
import { usePanelWidth } from "../hooks/usePanelWidth";
import { PRESETS } from "../components/ScopeControls";
import { selectionNode } from "../deepLink";
import { ALL_FRAMES, ALL_PAGES, frameDimensions, frameForNode } from "../frameScope";
import { inferPageId } from "../pageInference";
import type { BoardSection } from "../scene/pageBoard";
import type { ProjectView } from "./projectView";
import { nodeName } from "../state";
import type { DesignNode, SceneRequest } from "../types";
import { ViewerStatus } from "./ViewerStatus";
import { useWorkspaceView } from "./useWorkspaceView";

interface Props {
  project: ProjectView;
  nodes: DesignNode[];
  frames: DesignNode[];
  /** All pages board: its pages, each with its frames. */
  sections?: BoardSection[];
  frame?: DesignNode;
  selectedFrame?: DesignNode;
  preset: string;
  leftOpen: boolean;
  rightOpen: boolean;
  showNotice: boolean;
}

/** Own scene subscriptions at the canvas boundary so frame progress does not rerender chrome. */
export function SceneWorkspace({
  project,
  nodes,
  frames,
  sections,
  frame,
  selectedFrame,
  preset,
  leftOpen,
  rightOpen,
  showNotice,
}: Props) {
  const { snapshot, selection, selectScope } = project;
  const [retryKey, setRetryKey] = useState(0);
  const retryScene = useCallback(() => setRetryKey((key) => key + 1), []);
  const frameIds = useMemo(() => frames.map((entry) => entry.id), [frames]);
  const selectedNode = useMemo(
    () => selectionNode(nodes, selection.nodeId),
    [nodes, selection.nodeId],
  );
  const all = selection.frameId === ALL_FRAMES;
  const viewport = PRESETS.find((entry) => entry.key === preset) ?? PRESETS[0];
  const dimensions = frame ? frameDimensions(frame) : undefined;
  const projectId = snapshot?.projectId,
    revision = snapshot?.revision,
    frameId = frame?.id;
  const width = viewport.width || dimensions?.width,
    height = viewport.height || dimensions?.height,
    hugHeight = dimensions?.hugHeight;
  // Stable identity: the scene hooks key their work on this request.
  const request: SceneRequest | null = useMemo(
    () =>
      projectId !== undefined && revision !== undefined && frameId && width && height
        ? { projectId, revision, frameId, width, height, hugHeight }
        : null,
    [projectId, revision, frameId, width, height, hugHeight],
  );
  const { view, computed, source } = useWorkspaceView({
    snapshot,
    pageId: selection.pageId,
    frames,
    frame,
    selectedFrame,
    nodeId: selectedNode?.id ?? "",
    all,
    preset,
    viewport,
    request,
    retryKey,
  });
  const selectNode = useCallback(
    (nodeId: string) => selectScope({ nodeId }),
    [selectScope],
  );
  // Links and actions address a node on its own page, also from the All pages board.
  const resolveSelection = useCallback(
    (id: string) => {
      const pageId =
        selection.pageId === ALL_PAGES
          ? inferPageId(nodes, { frameId: ALL_FRAMES, nodeId: id }) || selection.pageId
          : selection.pageId;
      return {
        ...selection,
        pageId,
        nodeId: id,
        frameId: frameForNode(nodes, id, pageId)?.id ?? selection.frameId,
      };
    },
    [nodes, selection],
  );
  const everyPage = selection.pageId === ALL_PAGES;
  const [leftWidth, setLeftWidth] = usePanelWidth("left-panel-width");
  const [rightWidth, setRightWidth] = usePanelWidth("right-panel-width");
  // Chosen widths apply on wide layouts only (workspace.css); null keeps the responsive default.
  const panelWidths = {
    "--left-panel-width": leftWidth === null ? undefined : `${leftWidth}px`,
    "--right-panel-width": rightWidth === null ? undefined : `${rightWidth}px`,
  } as CSSProperties;
  const workspaceClass = [
    "workspace",
    leftWidth !== null && "left-sized",
    rightWidth !== null && "right-sized",
  ].filter(Boolean).join(" ");

  return (
    <NodeActionsProvider
      selection={selection}
      revision={snapshot?.revision}
      select={selectNode}
      resolveSelection={resolveSelection}
    >
      <div className={workspaceClass} style={panelWidths}>
        {leftOpen && (
          <aside className="left-panel">
            <Layers
              nodes={nodes}
              frameId={selection.frameId}
              frameIds={all ? frameIds : undefined}
              groups={everyPage ? sections : undefined}
              selectedId={selection.nodeId}
              select={selectNode}
            />
          </aside>
        )}
        {leftOpen && (
          <PanelResizer
            side="left"
            label="Resize layers panel"
            width={leftWidth}
            setWidth={setLeftWidth}
          />
        )}
        <CanvasView
          view={view}
          selectedId={selection.nodeId}
          select={selectNode}
          retry={retryScene}
          sections={everyPage ? sections : undefined}
          frameName={
            everyPage
              ? `All pages · ${sections?.length ?? 0} · ${frames.length} frames`
              : all
                ? `All frames · ${frames.length}`
                : frame
                  ? nodeName(frame)
                  : ""
          }
        />
        {rightOpen && (
          <PanelResizer
            side="right"
            label="Resize inspector"
            width={rightWidth}
            setWidth={setRightWidth}
          />
        )}
        {rightOpen && (
          <Inspector
            node={selectedNode}
            computed={computed}
            source={source}
            revision={snapshot?.revision}
          />
        )}
      </div>
      <ViewerStatus project={project} view={view} showNotice={showNotice} />
    </NodeActionsProvider>
  );
}
