import { useCallback, useEffect, useMemo, useState } from "react";
import { SceneWorkspace } from "./app/SceneWorkspace";
import { ScopeControls } from "./components/ScopeControls";
import { Topbar } from "./components/Topbar";
import { useProject } from "./hooks/useProject";
import { useProjectList } from "./hooks/useProjectList";
import { useProjectLive } from "./hooks/useProjectLive";
import { useAcceptedSnapshot, useSnapshot } from "./hooks/useSnapshot";
import { ALL_FRAMES, ALL_PAGES } from "./frameScope";
import { frameInScope } from "./pageInference";
import { nodeName } from "./state";
import { useAllPagesBoard } from "./engine/useEngine";
import type { BoardSection } from "./scene/pageBoard";
import { EMPTY_NODES, nodeIndex, snapshotNodeIndex } from "./nodeIndex";
import useLocalStorage from "./hooks/ultility/useLocalStorage";
import { usePanelVisibility } from "./hooks/usePanelVisibility";

const isBoolean = (value: unknown): value is boolean =>
  typeof value === "boolean";

/** Project navigation and shell preferences; scene progress stays in the workspace. */
export function App() {
  const list = useProjectList();
  const navigation = useProject(list.projects, list.loaded);
  const { selection } = navigation;
  const knownRevision =
    list.projects.find((entry) => entry.projectId === selection.projectId)
      ?.revision ?? null;
  const snapshotQuery = useSnapshot(selection.projectId, knownRevision);
  const { snapshot } = snapshotQuery;
  useAcceptedSnapshot(snapshot, snapshotQuery.validation, navigation.accepted);
  const live = useProjectLive(list, navigation, snapshotQuery);
  const selectScope = useCallback(
    (patch: Parameters<typeof navigation.selectScope>[1]) =>
      navigation.selectScope(snapshot, patch),
    [navigation.selectScope, snapshot],
  );
  const project = {
    snapshot,
    selection,
    selectScope,
    notice: navigation.notice,
    error: list.error || snapshotQuery.error,
    ...live,
  };
  const { leftOpen, rightOpen, toggleLeft, toggleRight } = usePanelVisibility();
  const [preset, setPreset] = useState("frame");
  const [isShowNoticeBar, setShowNoticeBar] = useLocalStorage(
    "is_show_notice_bar",
    false,
    isBoolean,
  );
  const nodes = snapshot?.nodes ?? EMPTY_NODES;
  const index = snapshot ? snapshotNodeIndex(snapshot) : nodeIndex(EMPTY_NODES);
  const pages = useMemo(
    () =>
      (index.childrenByParent.get(null) ?? EMPTY_NODES).filter(
        (node) => node.kind === "page",
      ),
    [index],
  );
  const allPages = selection.pageId === ALL_PAGES;
  const allPagesBoard = useAllPagesBoard();
  // All pages: every page's frames in page order, grouped into titled board sections.
  const { frames, sections } = useMemo(() => {
    const framesOf = (pageId: string) =>
      (index.childrenByParent.get(pageId) ?? EMPTY_NODES).filter(
        (node) => node.kind === "frame",
      );
    if (!allPages) return { frames: framesOf(selection.pageId), sections: undefined };
    const sections: BoardSection[] = pages.map((page) => ({
      id: page.id,
      name: nodeName(page),
      frameIds: framesOf(page.id).map((frame) => frame.id),
    }));
    return { frames: pages.flatMap((page) => framesOf(page.id)), sections };
  }, [index, pages, allPages, selection.pageId]);
  const frame = index.byId.get(selection.frameId);
  const selectedFrame = useMemo(
    () => frameInScope(nodes, selection.nodeId, selection.pageId),
    [nodes, selection.nodeId, selection.pageId],
  );
  // Canvas2D (chosen or fallen back to) has no All pages board, the default scope: show the
  // first page instead, replacing the board address so Back does not return to it.
  const firstPage = pages[0]?.id;
  const replaceScope = navigation.selectScope;
  useEffect(() => {
    if (allPages && !allPagesBoard && firstPage)
      replaceScope(
        snapshot,
        { pageId: firstPage, frameId: ALL_FRAMES, nodeId: "" },
        "replace",
      );
  }, [allPages, allPagesBoard, firstPage, replaceScope, snapshot]);

  return (
    <div
      className={`app-shell ${leftOpen ? "left-open" : ""} ${rightOpen ? "right-open" : ""}`}
    >
      <Topbar
        projects={list.projects}
        projectId={selection.projectId}
        activity={project.activity}
        loading={list.loading}
        failed={!!project.error}
        selectProject={project.selectProject}
        revision={
          snapshot?.projectId === selection.projectId
            ? snapshot.revision
            : undefined
        }
        refresh={project.retry}
        frameId={frame?.id ?? selectedFrame?.id ?? ""}
        leftOpen={leftOpen}
        rightOpen={rightOpen}
        toggleLeft={toggleLeft}
        toggleRight={toggleRight}
        isShowNoticeBar={isShowNoticeBar}
        setShowNoticeBar={setShowNoticeBar}
      />
      <ScopeControls
        pages={pages}
        frames={frames}
        selection={selection}
        select={project.selectScope}
        allPages={allPagesBoard}
        preset={preset}
        setPreset={setPreset}
      />
      <SceneWorkspace
        project={project}
        nodes={nodes}
        frames={frames}
        sections={sections}
        frame={frame}
        selectedFrame={selectedFrame}
        preset={preset}
        leftOpen={leftOpen}
        rightOpen={rightOpen}
        showNotice={isShowNoticeBar}
      />
    </div>
  );
}
