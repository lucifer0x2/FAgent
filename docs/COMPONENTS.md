# Components and instances (P24)

Reusable components use a frame with `component:{name}` and ordinary authored
children. Place an `instance` leaf with `componentId` and optional `overrides`
keyed by master descendant ID. The lists below follow `INSTANCE_KEYS` and
`OVERRIDE_KEYS` in `crates/scene-input/src/instances.rs`.

- Instance keys (all but `componentId` and `overrides` patch the expanded root):
  `componentId`, `overrides`, `name`, `x`, `y`, `width`, `height`, `minWidth`,
  `maxWidth`, `minHeight`, `maxHeight`, `visible`, `theme`, `fill`, `stroke`,
  `shadow`, `radius`, `opacity`, `states`, `clip`, `layout`.
- Override keys (per master descendant): `text`, `fill`, `visible`, `styleId`,
  `assetId`, `name`, `stroke`, `shadow`, `states`, `opacity`, `radius`, `clip`,
  `textDecoration`, `textTransform`, `textAlign`, `letterSpacing`, `textWrap`,
  `width`, `height`, `minWidth`, `maxWidth`, `minHeight`, `maxHeight`, `layout`.

`stroke` and `layout` merge key by key (a `layout` patch's `breakpoints` replace
the master's whole list) and `states` merges per state; every other key replaces
the master value. So `{"inner":{"width":164,"layout":{"justify":"center"}}}`
keeps the inner row's padding and gap, and an instance's own `layout:{"padding":8}`
moves the master's children. A patch changes the base rule only: at a width where a
master breakpoint applies, that full rule still wins, so patch `breakpoints` as well.
Font keys (`fontSize`, `lineHeight`, `fontId`, `fontWeight`, `fontStyle`), `x`/`y` of
descendants and structure stay inherited from the master. A merged size is validated
where the node is placed, so a `fill` inside a hug axis still fails under
`props.overrides.<id>.width`.

The master root is not an override target: set its props on the instance itself,
next to `componentId` (`overrides.<masterRootId>` fails and lists the instance
keys). A key `<nestedInstanceId>/<descendantId>` reaches through an instance nested
in the master (`btn/label`; deeper paths such as `card/btn/label` alike): expansion
hands the nested instance every outer override under its ID, and the outer patch
wins key by key over the nested instance's own override. Validation walks the path
segment by segment (each segment but the last must be an instance inside the master
named so far), so editing a nested master so that a path dangles fails that commit
with the closest path as `suggestion`. An instance root is
a frame and clips its children like any frame; set `clip:false` on the instance (or
an override `clip:false` on a descendant frame) to paint a child, stroke or shadow
past its edge. Full authoring rules are in `skill://fagent/props.md`. Render, inspect and image export use expanded nodes
with stable virtual IDs such as `row3/cmpRow-q`; writes to those IDs fail with
`INSTANCE_CHILD_READONLY`. Change the instance's `props.overrides`: `update_node`
replaces the whole object unless `merge:true`; OCD `@set x overrides={…}` merges per
descendant (`"<id>":null` removes one, `overrides=null` clears all, `{}` changes nothing).

In a row or column with `align:"stretch"` whose cross size hugs its content, a
cross-axis `fill` (`height:"fill"` in a row) stretches the child to the line. This is
how an instance replaces a fixed master height to match its tallest sibling; with a
definite cross size `fill` stays 100% of the parent.

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
