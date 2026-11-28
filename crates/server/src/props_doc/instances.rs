//! Minimal component authoring contract, outside the compact tool catalog. The key
//! lists come from the validator's constants, so they match what a commit accepts.
use fagent_storage::instances::OVERRIDE_KEYS;

fn keys(list: &[&str]) -> String {
    let keys: Vec<String> = list.iter().map(|key| format!("`{key}`")).collect();
    keys.join(", ")
}

pub(super) fn section() -> String {
    format!(
        "{INTRO}The `instance` keys listed above, except `componentId` and `overrides`, patch \
         the expanded master root: to change the root's fill, stroke, radius, size or \
         padding, write the key on the instance itself, next to `componentId` \
         (`{{\"componentId\":\"card\",\"fill\":\"#ffffff\",\"layout\":{{\"padding\":8}}}}`); \
         `stroke`, `layout` and `states` merge.\n\n\
         `overrides` is an object keyed by authored master descendant IDs, never the master \
         root's own ID. A key `<nestedInstanceId>/<descendantId>` (`btn/label`, deeper paths \
         alike) reaches into an instance nested in the master and wins over that instance's \
         own override. Each value is a partial props object accepting {}, validated for the \
         target's kind against the fully merged value; `layout` merges per key and its \
         `breakpoints` replace the master's list; at a width where a master breakpoint applies, \
         that full rule wins over the patched base layout, so patch `breakpoints` too. {REST}",
        keys(OVERRIDE_KEYS)
    )
}

const INTRO: &str = "\n## Components and instances\n\n\
     A reusable master is a `frame` with `component: {name: \"TableRow\"}`. Its children \
     are authored normally. Masters may use fill/hug sizing; a rendered root frame \
     still needs a numeric width. Create an `instance` leaf with `componentId` pointing \
     to the master. It inherits the master's layout and paint; its own name, visibility, \
     position, sizing, paint and layout replace or patch the corresponding root \
     properties. ";

const REST: &str = "Stroke objects merge; shadow arrays \
     replace all layers; states merge per state and visual property (including stroke). \
     Patching hover preserves focus/pressed and unpatched hover properties. State shadow \
     remains unsupported. Image \
     references use `assetId`; token references use `{tokenId: \"...\"}`. Font keys, \
     `x`/`y`, structure and component references cannot be overridden. \
     Master root/child states are inherited by the corresponding expanded nodes. \
     `update_node` replaces the complete `overrides` object (`merge:true` patches entries; \
     OCD `@set` always patches: `overrides=null` clears all, `{}` changes nothing).\n\n\
     Frames and instances accept `theme: \"night\"`: the name must occur in a token's \
     themes (or be `default`), otherwise `UNKNOWN_THEME`. The nearest scope wins; \
     render/export theme is the global fallback. Expanded instances inherit scopes at \
     their placement, never the master's ancestors. A master root theme is inherited, \
     and an explicit instance theme replaces it. Nested frame scopes still win below it. \
     Context node `effectiveTheme` and `resolvedProps` describe that node's scope; resource \
     token/style `resolvedValue` describes the requested global fallback theme. Scene and \
     inspect nodes also expose `effectiveTheme`. Authored source remains unchanged.\n\n\
     Expanded children have stable IDs `<instanceId>/<masterChildId>` and are read-only: \
     update the instance's `props.overrides` instead (`INSTANCE_CHILD_READONLY`). \
     Instances cannot have authored children. Nesting is limited to 8 component levels; \
     cycles fail with `INSTANCE_CYCLE`. Deleting a referenced master fails with \
     `COMPONENT_IN_USE`, `count` and up to 20 `instanceIds`. Component subtrees cannot \
     carry links, flow starts or animations.\n\n\
     `get_design_context` defaults to `instances: \"reference\"`: one definition per \
     referenced master in `components`, instances as leaves and optional computed \
     child bounds/lines. Choose `instances: \"expand\"` for the full virtual tree. \
     `get_outline` and `get_subtree` show instance leaves with `componentId` and \
     `childCount`; scope a virtual child directly to read it. `find_nodes` accepts \
     `kinds: [\"instance\"]` and searches overridden child text using virtual IDs.\n";
