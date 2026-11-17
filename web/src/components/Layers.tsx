import { memo, useLayoutEffect, useMemo, useRef, useState, type FocusEvent } from "react";
import type { BoardSection } from "../scene/pageBoard";
import { nodeIndex } from "../nodeIndex";
import type { DesignNode } from "../types";
import { layerSelectionPath } from "../layerSelection";
import { LayerRow } from "./LayerRow";
import { LayerGroupRow } from "./layers/LayerGroupRow";
import { useNodeActions } from "./NodeActionsProvider";
import {
  flattenLayers,
  revealTarget,
  selectionPathIds,
  type LayerListRow,
} from "./layers/flattenLayers";
import { useLayerExpansion } from "./layers/useLayerExpansion";
import { useVirtualRows } from "./layers/useVirtualRows";

/** Must match `.layer-row` / `.layer-group-heading` height in workspace.css. */
const ROW_HEIGHT = 33;

export const Layers = memo(function Layers({
  nodes,
  frameId,
  frameIds,
  groups,
  selectedId,
  select,
}: {
  nodes: DesignNode[];
  frameId: string;
  frameIds?: string[];
  /** All pages board: frames under a collapsible heading per page. */
  groups?: BoardSection[];
  selectedId: string;
  select: (id: string) => void;
}) {
  const index = nodeIndex(nodes);
  const actions = useNodeActions();
  const pathIds = useMemo(
    () => selectionPathIds(layerSelectionPath(nodes, selectedId)),
    [nodes, selectedId],
  );
  const pathKey = pathIds.join("\n");
  const selectedRowId = pathIds.at(-1);
  const roots = useMemo(
    () => (frameIds ?? [frameId]).flatMap((id) => index.byId.get(id) ?? []),
    [frameIds, frameId, index],
  );
  const { collapsed, isGroupOpen, toggleGroup, toggleNode } = useLayerExpansion(
    groups,
    pathIds,
  );
  // Re-flatten only when the set of open pages changes, not on every selection that moves
  // between frames (isGroupOpen's identity follows the selected frame).
  const openKey = groups?.map((group) => (isGroupOpen(group) ? 1 : 0)).join("") ?? "";
  const rows = useMemo(
    () => flattenLayers({ index, roots, groups, isGroupOpen, collapsed }),
    [index, roots, groups, collapsed, openKey],
  );
  const target = useMemo(() => revealTarget(rows, pathIds), [rows, pathIds]);

  const list = useRef<HTMLUListElement>(null);
  const { start, end, reveal } = useVirtualRows(list, rows.length, ROW_HEIGHT);

  // A new selection (or a revision, or the panel shown again) scrolls its row into view once
  // that row exists; expanding or collapsing other rows afterwards leaves the scroll alone.
  const pending = useRef(true);
  useLayoutEffect(() => {
    pending.current = true;
  }, [pathKey, nodes]);
  useLayoutEffect(() => {
    if (!pending.current || target === undefined) return;
    pending.current = false;
    reveal(target);
  }, [pathKey, nodes, target, reveal]);

  // Keep the focused row mounted when it scrolls out of the window, so focus is not lost.
  const [focusedKey, setFocusedKey] = useState<string>();
  const focusedIndex = useMemo(
    () => (focusedKey ? rows.findIndex((row) => row.key === focusedKey) : -1),
    [rows, focusedKey],
  );
  const visible: number[] = [];
  for (let i = start; i < end; i++) visible.push(i);
  if (focusedIndex >= 0 && (focusedIndex < start || focusedIndex >= end))
    visible.push(focusedIndex);

  const renderRow = (row: LayerListRow, i: number) =>
    row.kind === "group" ? (
      <LayerGroupRow
        key={row.key}
        group={row.group}
        order={row.order}
        open={row.open}
        top={i * ROW_HEIGHT}
        toggle={toggleGroup}
      />
    ) : (
      <LayerRow
        key={row.key}
        node={row.node}
        depth={row.depth}
        top={i * ROW_HEIGHT}
        expandable={row.expandable}
        expanded={row.expanded}
        selected={row.node.id === selectedRowId}
        select={select}
        toggle={toggleNode}
        openActions={actions?.open}
      />
    );

  return (
    <section className="layers-content" aria-label="Layers">
      <div className="section-heading">
        <span>Layers</span>
        <span className="subtle">Read only</span>
      </div>
      <ul
        ref={list}
        className="layers-list"
        style={{ height: rows.length * ROW_HEIGHT }}
        onFocus={(event) => setFocusedKey(rowKey(event.target))}
        onBlur={(event: FocusEvent) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null))
            setFocusedKey(undefined);
        }}
      >
        {visible.map((i) => renderRow(rows[i], i))}
      </ul>
      {!rows.length && (
        <p className="panel-empty">Choose a frame to see its layers.</p>
      )}
      <div className="panel-footer">
        Changes from your agent appear here automatically.
      </div>
    </section>
  );
});

function rowKey(element: EventTarget) {
  return (element as HTMLElement).closest?.("[data-row-key]")?.getAttribute("data-row-key") ?? undefined;
}
