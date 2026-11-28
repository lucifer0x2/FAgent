# Components and instances (P24)

Reusable components use a frame with `component:{name}` and ordinary authored
children. Place an `instance` leaf with `componentId` and optional flat `overrides`
keyed by master descendant ID. The lists below follow `INSTANCE_KEYS` and
`OVERRIDE_KEYS` in `crates/scene-input/src/instances.rs`.

- Instance keys (all but `componentId` and `overrides` patch the expanded root):
  `componentId`, `overrides`, `name`, `x`, `y`, `width`, `height`, `minWidth`,
  `maxWidth`, `minHeight`, `maxHeight`, `visible`, `theme`, `fill`, `stroke`,
  `shadow`, `radius`, `opacity`, `states`, `clip`.
- Override keys (per master descendant): `text`, `fill`, `visible`, `styleId`,
  `assetId`, `name`, `stroke`, `shadow`, `states`, `opacity`, `radius`, `clip`,
  `textDecoration`, `textTransform`, `textAlign`, `letterSpacing`, `textWrap`.

`stroke` merges as an object and `states` merges per state; every other key
replaces the master value. Font keys (`fontSize`, `lineHeight`, `fontId`,
`fontWeight`, `fontStyle`), layout and structure stay inherited from the master.
The master root is not an override target: set its props on the instance itself
(`overrides.<masterRootId>` fails and lists the instance keys). An instance root is
a frame and clips its children like any frame; set `clip:false` on the instance (or
an override `clip:false` on a descendant frame) to paint a child, stroke or shadow
past its edge. Full authoring rules are in `skill://fagent/props.md`. Render, inspect and image export use expanded nodes
with stable virtual IDs such as `row3/cmpRow-q`; writes to those IDs fail with
`INSTANCE_CHILD_READONLY`. Change the instance's whole `props.overrides` object.

`get_design_context` defaults to `instances:"reference"`: each master referenced
within `depth` appears once as a paged `components` entry `{name, rootId, nodes}`
(including nested dependencies; see `SERVER.md`), while
instance leaves expose `componentId`, `overrides`, `childCount` and, when computed,
`computed:{bounds,children:{"row3/cmpRow-q":{bounds,lines}}}`. Choose
`instances:"expand"` for all virtual nodes. Compact resources cover definitions and
overrides, including style/token/font/asset dependencies across themes. Outline and
subtree keep instances as leaves; a virtual-child scope reads that child directly.
`find_nodes` supports `kinds:["instance"]`, `properties:["componentId"]` and, with
`instances:"expand"` (or a scope on the instance), effective overridden child text; its
default `"reference"` skips virtual children and counts them in `skippedInstanceMatches`.
Snapshot/bundle storage remains authored. Cycles/depth >8,
invalid override kinds/refs and an expanded project >200,000 nodes are rejected
atomically; deleting an in-use master returns `COMPONENT_IN_USE` with `count` and
up to 20 `instanceIds`. No new MCP tool is added.

## Interaction states

Expansion preserves the master root's `states` on each placed instance and each
master descendant's own declarations on its virtual child. Theme refs resolve
after expansion. A child never receives a parent's state override implicitly.
An instance may author its own root `states` and `overrides.<childId>.states`; both
merge per state over the master declarations (a key set for `hover` replaces only
that key of the master's `hover`), and the merged result is validated like authored
`states`.

`statesMode:"resolved"` exposes differing variants on instance roots, virtual
nodes in expand mode, and nodes inside reference `components` definitions.
Compact resource reads retain tokens/styles referenced only by these variants.
An override `visible:false` remains hidden in every rendered state, while keeping
layout space. State preview and geometry semantics follow
[Interaction states](INTERACTION-STATES.md).
