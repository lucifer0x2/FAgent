import { selectionNode } from "./deepLink.ts";
import { ALL_FRAMES, ALL_PAGES, frameForNode } from "./frameScope.ts";
import { nodeIndex } from "./nodeIndex.ts";
import type { DesignNode, Selection } from "./types.ts";

const isPage = (node?: DesignNode) =>
  node?.kind === "page" && node.parentId === null;

/**
 * A link without `page` names its page through the addressed root frame, or failing
 * that through the node's ancestry (including virtual instance children). Returns ""
 * when neither exists, so callers keep their first-page fallback and notices.
 */
export function inferPageId(
  nodes: DesignNode[],
  { frameId, nodeId }: Pick<Selection, "frameId" | "nodeId">,
): string {
  const { byId } = nodeIndex(nodes);
  const frame = frameId === ALL_FRAMES ? undefined : byId.get(frameId);
  if (frame?.kind === "frame" && isPage(byId.get(frame.parentId ?? "")))
    return frame.parentId ?? "";
  let node = selectionNode(nodes, nodeId);
  const seen = new Set<string>();
  while (node && !seen.has(node.id)) {
    if (isPage(node)) return node.id;
    seen.add(node.id);
    node =
      byId.get(node.parentId ?? "") ??
      selectionNode(nodes, node.parentId ?? "");
  }
  return "";
}

/**
 * The root frame holding `nodeId` within the page scope: the selected page, or on the
 * All pages board the node's own page.
 */
export function frameInScope(nodes: DesignNode[], nodeId: string, pageId: string) {
  const page =
    pageId === ALL_PAGES ? inferPageId(nodes, { frameId: ALL_FRAMES, nodeId }) : pageId;
  return page ? frameForNode(nodes, nodeId, page) : undefined;
}
