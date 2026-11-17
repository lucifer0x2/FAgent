import assert from "node:assert/strict";
import test from "node:test";
import {
  flattenLayers,
  revealTarget,
  selectionPathIds,
  type FlattenInput,
  type LayerListRow,
} from "../src/components/layers/flattenLayers.ts";
import { layerSelectionPath } from "../src/layerSelection.ts";
import { nodeIndex } from "../src/nodeIndex.ts";
import type { BoardSection } from "../src/scene/pageBoard.ts";
import type { DesignNode } from "../src/types.ts";

const node = (
  id: string, parentId: string | null, kind = "group", sortKey = 0, props = {},
): DesignNode => ({ id, parentId, kind, sortKey, props }) as DesignNode;
const nodes = [
  node("page1", null, "page", 0), node("page2", null, "page", 1),
  node("f1", "page1", "frame", 0),
  node("g1", "f1", "group", 0), node("leaf", "g1", "rectangle", 0),
  node("r1", "f1", "rectangle", 1),
  node("f2", "page1", "frame", 1),
  node("f3", "page2", "frame", 0),
  node("inst", "f3", "instance", 0, { componentId: "master" }),
  node("master", null, "component", 2), node("label", "master", "text"),
];
const index = nodeIndex(nodes);
const byId = (id: string) => index.byId.get(id)!;
const groups: BoardSection[] = [
  { id: "page1", name: "One", frameIds: ["f1", "f2"] },
  { id: "page2", name: "Two", frameIds: ["f3", "gone"] },
];
const input = (patch: Partial<FlattenInput> = {}): FlattenInput => ({
  index,
  roots: [byId("f1")],
  isGroupOpen: () => true,
  collapsed: new Set(),
  ...patch,
});
const shape = (rows: LayerListRow[]) =>
  rows.map((row) =>
    row.kind === "group"
      ? `group:${row.group.id}:${row.order}:${row.open ? "open" : "closed"}`
      : `${row.node.id}@${row.depth}${row.expandable ? (row.expanded ? "-" : "+") : ""}`,
  );

test("an expanded tree lists every node depth-first with its depth", () => {
  assert.deepEqual(shape(flattenLayers(input())), ["f1@0-", "g1@1-", "leaf@2", "r1@1"]);
  assert.deepEqual(
    shape(flattenLayers(input({ roots: [byId("f1"), byId("f2")] }))),
    ["f1@0-", "g1@1-", "leaf@2", "r1@1", "f2@0"],
  );
  assert.deepEqual(flattenLayers(input({ roots: [] })), []);
});

test("collapsed nodes keep their row but skip their descendants", () => {
  assert.deepEqual(
    shape(flattenLayers(input({ collapsed: new Set(["g1"]) }))),
    ["f1@0-", "g1@1+", "r1@1"],
  );
  assert.deepEqual(shape(flattenLayers(input({ collapsed: new Set(["f1"]) }))), ["f1@0+"]);
  // Collapsing a leaf changes nothing visible: it has no disclosure.
  assert.deepEqual(
    shape(flattenLayers(input({ collapsed: new Set(["leaf"]) }))),
    ["f1@0-", "g1@1-", "leaf@2", "r1@1"],
  );
});

test("page groups list their frames at depth 1 and closed groups hide them", () => {
  const open = flattenLayers(input({ groups }));
  assert.deepEqual(shape(open), [
    "group:page1:0:open", "f1@1-", "g1@2-", "leaf@3", "r1@2", "f2@1",
    // A frame ID missing from the snapshot ("gone") gets no row; an instance has no
    // authored children, so no disclosure.
    "group:page2:1:open", "f3@1-", "inst@2",
  ]);
  const closed = flattenLayers(input({ groups, isGroupOpen: (group) => group.id === "page2" }));
  assert.deepEqual(shape(closed), ["group:page1:0:closed", "group:page2:1:open", "f3@1-", "inst@2"]);
  assert.deepEqual(new Set(open.map((row) => row.key)).size, open.length, "row keys are unique");
  // Without groups (or an empty list) `roots` is used.
  assert.deepEqual(shape(flattenLayers(input({ groups: [] }))), ["f1@0-", "g1@1-", "leaf@2", "r1@1"]);
});

test("cyclic parents terminate", () => {
  const cyclic = [node("a", "b"), node("b", "a")];
  const cycleIndex = nodeIndex(cyclic);
  const rows = flattenLayers({
    index: cycleIndex, roots: [cycleIndex.byId.get("a")!], isGroupOpen: () => true, collapsed: new Set(),
  });
  assert.deepEqual(shape(rows), ["a@0-", "b@1-"]);
});

test("selectionPathIds lists the frame first and the selected node last", () => {
  assert.deepEqual(selectionPathIds(layerSelectionPath(nodes, "leaf")), ["f1", "g1", "leaf"]);
  assert.deepEqual(selectionPathIds(layerSelectionPath(nodes, "f2")), ["f2"]);
  assert.deepEqual(selectionPathIds(layerSelectionPath(nodes, "inst/label")), ["f3", "inst", "inst/label"]);
  assert.deepEqual(selectionPathIds(undefined), []);
  assert.deepEqual(selectionPathIds(layerSelectionPath(nodes, "gone")), []);
});

test("revealTarget picks the selected row, or its deepest visible ancestor", () => {
  const path = selectionPathIds(layerSelectionPath(nodes, "leaf"));
  const rowId = (rows: LayerListRow[], at: number | undefined) =>
    at === undefined ? undefined : (rows[at] as Extract<LayerListRow, { kind: "node" }>).node.id;
  const expanded = flattenLayers(input({ groups }));
  assert.equal(rowId(expanded, revealTarget(expanded, path)), "leaf");
  const collapsed = flattenLayers(input({ groups, collapsed: new Set(["g1"]) }));
  assert.equal(rowId(collapsed, revealTarget(collapsed, path)), "g1");
  // A closed page has no row for its frames; group headings are never targets.
  const closed = flattenLayers(input({ groups, isGroupOpen: () => false }));
  assert.equal(revealTarget(closed, path), undefined);
  assert.equal(revealTarget(expanded, []), undefined);
});

test("a virtual instance child is revealed through its instance row", () => {
  const rows = flattenLayers(input({ groups }));
  const path = selectionPathIds(layerSelectionPath(nodes, "inst/label"));
  const at = revealTarget(rows, path)!;
  assert.equal(rows[at].key, "inst");
});
