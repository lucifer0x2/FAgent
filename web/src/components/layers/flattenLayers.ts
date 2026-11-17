import { EMPTY_NODES, type NodeIndex } from "../../nodeIndex.ts";
import type { BoardSection } from "../../scene/pageBoard.ts";
import type { DesignNode } from "../../types.ts";
import type { LayerSelectionPath } from "../../layerSelection.ts";

/** One line of the virtualized Layers list: a page heading or a node at a tree depth. */
export type LayerListRow =
  | {
      kind: "group";
      key: string;
      group: BoardSection;
      order: number;
      open: boolean;
    }
  | {
      kind: "node";
      key: string;
      node: DesignNode;
      depth: number;
      expandable: boolean;
      expanded: boolean;
    };

export interface FlattenInput {
  index: NodeIndex;
  /** Top-level frames when there are no page groups. */
  roots: readonly DesignNode[];
  /** All pages board: one heading per page, its frames below at depth 1. */
  groups?: readonly BoardSection[];
  isGroupOpen: (group: BoardSection) => boolean;
  /** Node rows are expanded unless listed here. */
  collapsed: ReadonlySet<string>;
}

/** The rows currently visible in tree order, honouring collapsed nodes and closed pages. */
export function flattenLayers(input: FlattenInput): LayerListRow[] {
  const rows: LayerListRow[] = [];
  const { index, groups } = input;
  if (groups?.length) {
    groups.forEach((group, order) => {
      const open = input.isGroupOpen(group);
      rows.push({ kind: "group", key: `group:${group.id}`, group, order, open });
      if (!open) return;
      const frames = group.frameIds.flatMap((id) => index.byId.get(id) ?? []);
      appendTree(rows, frames, 1, input);
    });
  } else appendTree(rows, input.roots, 0, input);
  return rows;
}

/** Depth-first, iterative so very deep or very wide trees cannot overflow the stack. */
function appendTree(
  rows: LayerListRow[],
  roots: readonly DesignNode[],
  depth: number,
  { index, collapsed }: FlattenInput,
) {
  const stack: { node: DesignNode; depth: number }[] = [];
  for (let i = roots.length - 1; i >= 0; i--) stack.push({ node: roots[i], depth });
  const seen = new Set<string>();
  while (stack.length) {
    const item = stack.pop()!;
    const { node } = item;
    // Malformed cyclic parents would otherwise loop forever.
    if (seen.has(node.id)) continue;
    seen.add(node.id);
    const children = index.childrenByParent.get(node.id) ?? EMPTY_NODES;
    const expanded = !collapsed.has(node.id);
    rows.push({
      kind: "node",
      key: node.id,
      node,
      depth: item.depth,
      expandable: children.length > 0,
      expanded,
    });
    if (!expanded) continue;
    for (let i = children.length - 1; i >= 0; i--)
      stack.push({ node: children[i], depth: item.depth + 1 });
  }
}

/** Root-to-leaf ids of the selection (the frame first, the selected node last). */
export function selectionPathIds(path: LayerSelectionPath | undefined): string[] {
  const ids: string[] = [];
  for (let step = path; step; step = step.child) ids.push(step.id);
  return ids;
}

/**
 * The row to reveal: the deepest selection ancestor that has a row. A virtual instance child
 * has none of its own, so its instance row stands in.
 */
export function revealTarget(
  rows: readonly LayerListRow[],
  pathIds: readonly string[],
): number | undefined {
  if (!pathIds.length) return undefined;
  const depthOf = new Map(pathIds.map((id, depth) => [id, depth]));
  let best: number | undefined;
  let bestDepth = -1;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    if (row.kind !== "node") continue;
    const depth = depthOf.get(row.node.id);
    if (depth !== undefined && depth > bestDepth) {
      best = i;
      bestDepth = depth;
      if (depth === pathIds.length - 1) break;
    }
  }
  return best;
}
