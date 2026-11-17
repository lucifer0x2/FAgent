import assert from "node:assert/strict";
import test from "node:test";
import { navigationLink, readNavigation, readNodeLink } from "../src/deepLink.ts";
import { ALL_FRAMES, ALL_PAGES } from "../src/frameScope.ts";
import { frameInScope } from "../src/pageInference.ts";
import { projectSelection } from "../src/projectSelection.ts";
import { reconcileSelection } from "../src/state.ts";
import type { DesignNode, Selection, Snapshot } from "../src/types.ts";

const node = (
  id: string, parentId: string | null, kind: string, sortKey = 0, props = {},
): DesignNode => ({ id, parentId, kind, sortKey, props }) as DesignNode;
const snapshot = {
  projectId: "p",
  revision: 4,
  nodes: [
    node("page1", null, "page", 0), node("page2", null, "page", 1),
    node("f1", "page1", "frame"), node("text1", "f1", "text"),
    node("f2", "page2", "frame"), node("group2", "f2", "group"),
    node("inst", "group2", "instance", 0, { componentId: "master" }),
    // A component master outside every page.
    node("master", null, "component", 2), node("label", "master", "text"),
  ],
} as Snapshot;
const board: Selection = { projectId: "p", pageId: ALL_PAGES, frameId: ALL_FRAMES, nodeId: "" };

test("frameInScope finds the root frame on the All pages board and within a page", () => {
  assert.equal(frameInScope(snapshot.nodes, "text1", ALL_PAGES)?.id, "f1");
  assert.equal(frameInScope(snapshot.nodes, "group2", ALL_PAGES)?.id, "f2");
  assert.equal(frameInScope(snapshot.nodes, "f2", ALL_PAGES)?.id, "f2");
  assert.equal(frameInScope(snapshot.nodes, "text1", "page1")?.id, "f1");
  // A normal page scope does not reach another page's frames.
  assert.equal(frameInScope(snapshot.nodes, "group2", "page1"), undefined);
  // Virtual instance children resolve through their instance.
  assert.equal(frameInScope(snapshot.nodes, "inst/label", ALL_PAGES)?.id, "f2");
  assert.equal(frameInScope(snapshot.nodes, "inst/label", "page2")?.id, "f2");
  assert.equal(frameInScope(snapshot.nodes, "inst/label", "page1"), undefined);
  // Unknown, page-less (component master) and page nodes have no frame.
  for (const id of ["gone", "", "label", "master", "page1"])
    assert.equal(frameInScope(snapshot.nodes, id, ALL_PAGES), undefined, id);
});

test("reconcileSelection keeps the All pages board and a layer of any page", () => {
  assert.equal(reconcileSelection(snapshot, board), board);
  for (const nodeId of ["text1", "group2", "inst/label", "f1"]) {
    const kept = { ...board, nodeId };
    assert.equal(reconcileSelection(snapshot, kept), kept, nodeId);
  }
  // The board has no single frame: an unknown frame value collapses to ALL_FRAMES.
  assert.deepEqual(reconcileSelection(snapshot, { ...board, frameId: "gone", nodeId: "text1" }), {
    ...board, nodeId: "text1",
  });
});

test("a frame addressed on the All pages board stays selected on its own page", () => {
  // As the Frame selector does on the board: the frame opens on its page, keeping its layer.
  assert.deepEqual(reconcileSelection(snapshot, { ...board, frameId: "f1", nodeId: "text1" }), {
    projectId: "p", pageId: "page1", frameId: "f1", nodeId: "text1",
  });
  assert.deepEqual(reconcileSelection(snapshot, { ...board, frameId: "f2" }), {
    projectId: "p", pageId: "page2", frameId: "f2", nodeId: "",
  });
  // A layer of another frame falls back to the addressed frame.
  assert.deepEqual(reconcileSelection(snapshot, { ...board, frameId: "f2", nodeId: "text1" }), {
    projectId: "p", pageId: "page2", frameId: "f2", nodeId: "f2",
  });
  // A `page=:all:&frame=F` link reports no removal; a frame that is gone still does.
  const link = readNavigation("http://h/?project=p&page=%3Aall%3A&frame=f2");
  const opened = projectSelection({ selection: link, notice: "" }, { type: "accepted", snapshot });
  assert.deepEqual(opened.selection, { projectId: "p", pageId: "page2", frameId: "f2", nodeId: "" });
  assert.equal(opened.notice, "");
  const missing = projectSelection(
    { selection: { ...link, frameId: "gone" }, notice: "" },
    { type: "accepted", snapshot },
  );
  assert.deepEqual(missing.selection, board);
  assert.match(missing.notice, /selected frame was removed/);
});

test("reconcileSelection clears an unknown or page-less node on the All pages board", () => {
  for (const nodeId of ["deleted", "label", "inst/missing"])
    assert.deepEqual(reconcileSelection(snapshot, { ...board, nodeId }), board, nodeId);
});

test("switching project keeps the All pages board but drops the old node", () => {
  const other = { ...board, projectId: "q", nodeId: "text1" };
  assert.deepEqual(reconcileSelection(snapshot, other), board);
  // A project without pages has no board: fall back to the empty page scope.
  const pageless = { ...snapshot, nodes: snapshot.nodes.filter((n) => n.kind !== "page") };
  assert.deepEqual(reconcileSelection(pageless, { ...board, nodeId: "text1" }), {
    projectId: "p", pageId: "", frameId: ALL_FRAMES, nodeId: "",
  });
});

test("an All pages deep link round-trips through the URL and survives reconciliation", () => {
  const selection = { ...board, nodeId: "inst/label" };
  const href = navigationLink("http://127.0.0.1:5173/?scene=server#kept", selection);
  const url = new URL(href);
  assert.match(url.search, /[?&]page=%3Aall%3A(&|$)/);
  assert.match(url.search, /[?&]frame=%3Aall%3A(&|$)/);
  assert.equal(url.searchParams.get("scene"), "server");
  assert.equal(url.hash, "#kept");
  const read = readNavigation(href);
  assert.deepEqual(read, selection);
  assert.deepEqual(readNodeLink(href), selection);
  assert.equal(reconcileSelection(snapshot, read), read);
  // A hand-written link with the encoded value reads the same.
  assert.deepEqual(
    readNavigation("http://h/?project=p&page=%3Aall%3A&frame=%3Aall%3A&node=text1"),
    { ...board, nodeId: "text1" },
  );
});

test("an address without a page opens the All pages board and writes it back", () => {
  // Fresh open or a project switch: no page, frame or node.
  const bare = readNavigation("http://h/?project=p");
  assert.deepEqual(reconcileSelection(snapshot, bare), board);
  assert.deepEqual(reconcileSelection(snapshot, { ...board, pageId: "" }), board);
  // The default is written into the URL, so a reload reads the same board.
  const href = navigationLink("http://h/?project=p", reconcileSelection(snapshot, bare));
  assert.equal(new URL(href).searchParams.get("page"), ALL_PAGES);
  assert.equal(new URL(href).searchParams.get("frame"), ALL_FRAMES);
  const reloaded = readNavigation(href);
  assert.deepEqual(reloaded, board);
  assert.equal(reconcileSelection(snapshot, reloaded), reloaded);
  // A partial link still opens its target's page; an explicit page still wins.
  assert.deepEqual(reconcileSelection(snapshot, readNavigation("http://h/?project=p&frame=f2")), {
    projectId: "p", pageId: "page2", frameId: "f2", nodeId: "",
  });
  assert.deepEqual(reconcileSelection(snapshot, readNavigation("http://h/?project=p&node=text1")), {
    projectId: "p", pageId: "page1", frameId: ALL_FRAMES, nodeId: "text1",
  });
  assert.deepEqual(reconcileSelection(snapshot, readNavigation("http://h/?project=p&page=page2")), {
    projectId: "p", pageId: "page2", frameId: ALL_FRAMES, nodeId: "",
  });
  // A project without pages has no board.
  const pageless = { ...snapshot, nodes: snapshot.nodes.filter((n) => n.kind !== "page") };
  assert.deepEqual(reconcileSelection(pageless, bare), {
    projectId: "p", pageId: "", frameId: ALL_FRAMES, nodeId: "",
  });
});

test("selecting a frame or layer from the default board navigates as on an explicit board", () => {
  const scope = (patch: Partial<Selection>) =>
    projectSelection({ selection: board, notice: "" }, { type: "scope", snapshot, patch }).selection;
  // Layers / canvas: the node's own page and frame.
  assert.deepEqual(scope({ pageId: "page2", frameId: "f2", nodeId: "group2" }), {
    projectId: "p", pageId: "page2", frameId: "f2", nodeId: "group2",
  });
  // Frame selector on the board: the frame opens on its page.
  assert.deepEqual(scope({ pageId: "page1", frameId: "f1", nodeId: "f1" }), {
    projectId: "p", pageId: "page1", frameId: "f1", nodeId: "f1",
  });
  // A layer kept on the board.
  assert.deepEqual(scope({ nodeId: "text1" }), { ...board, nodeId: "text1" });
});
