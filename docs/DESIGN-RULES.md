# Shared design protocol for two AI sessions — draft v1

Date: 2026-10-29. Applies to AI that creates/edits or reads FAgent designs.
This is an **agent convention**, not yet a policy enforced by the backend. Apply it only after
the project's manifest has been filled in/finalized; do not pick a palette or font from the draft.
Foundations and limits: [resolution](RESOLUTION.md), [components](COMPONENTS.md).

## 1. One canonical manifest per project

There is only one authoritative copy, designated by the user/design lead. Both AIs
receive the same content or the same path/ID. The template below is a working record, **not**
a JSON schema for `apply_batch` or a new field in the project:

```yaml
projectId: <ID verified with list_projects>
rulesVersion: <rules version>
verifiedRevision: <project revision when the registry was checked>
theme: <default or a declared name>
rulesSource: <assigned file or rulesFrameId + note IDs>
semanticTokens: <role -> token ID; background, text, action, border, status...>
spacingTokens: <role -> dimension token ID>
textStyles: <role -> style ID; heading, body, label, caption...>
fonts: <font ID, family, weight, style, content hash and approved fallback>
components: <role -> master ID, allowed overrides, approved variants>
viewports: <width/height of the sizes that must be checked>
exceptions: <scope/property, reason, who decided>
systemOwner: <person/session assigned to change shared resources>
```

IDs/values in the project are the canonical data. The manifest only specifies roles and scope of
use; if two sources conflict, re-read and resolve before the related change.
Do not use `verifiedRevision` as a store of old snapshots: the read API only has the current revision.
A new session must check the registry at the latest revision and update the corresponding evidence.

If the registry lacks a needed role, send a concrete proposal with name, type, value,
theme, font/style and impact scope to the assigned lead; do not build a new
system inside a frame on your own. Work independent of that decision can continue.

## 2. Mandatory bootstrap in every session

1. Read the manifest and note projectId/rulesVersion/theme. For MCP-only: read the assigned rules
   frame with `get_design_context {projectId, frameId, notes:"all"}`;
   read every page/part. There is currently no `get_design_rules` tool.
2. Read the MCP connect instructions; through `resources/read` read
   `skill://fagent/props.md` and `skill://fagent/author-frame.md` or
   `skill://fagent/implement-frame.md`. Only call capabilities the server actually publishes.
3. `list_projects` to verify the right project; `get_outline`/`find_nodes` to identify the frame.
   Do not create a new project just because you forgot the ID. Do not put demo data into a real project.
4. Read `list_tokens`, `list_styles`, `list_fonts` by the recorded IDs (at most
   100 IDs each time), and the resources to be discovered. Take revision R from the first response; pass
   `revision:R` to the following reads, following the cursor to the end. If `REVISION_UNAVAILABLE`
   or `INVALID_CURSOR`, start a new read pass; do not stitch different revisions together.
5. Read `get_design_context` for the working frame with the same R/theme,
   `compact:true`, `instances:"reference"`, `notes:"all"` and the agreed viewport.
   Read all pagination; compact only returns referenced resources and does not replace the
   registry read if you need to pick more tokens/styles/components.
6. Check font record/hash/status and dependency masters. Start once you understand the
   authored refs, resolved values, notes and related exceptions.

The session-opening prompt can use the following in essence, filled with real information:

> Work on project `<projectId>`, scope `<frameId>`. Read the protocol and manifest
> `<canonical source>` version `<rulesVersion>` before writing. Use the agreed token/style/font/
> master IDs; do not pick a palette/font or edit the shared system outside the assigned
> scope. Read data at the same revision/theme, report gaps and exceptions.

This file does not appear in MCP resources on its own. The coordinator must hand the pointer
to both sessions. Rules notes should stay unresolved and be marked "standing rule"; use
`notes:"all"` so no rule is lost if someone already resolved it. Notes on other pages must
be read separately through the rules frame; the working frame's context does not inherit them.

## 3. Authoring rules

- **Color:** use the agreed semantic token IDs via `fill:{"tokenId":"<id>"}` or the agreed
  fill style. Keep aliases to primitives if the registry has them; do not copy the
  resolved hex as a literal into each node. Exceptions must be in the manifest.
- **Typography:** use `styleId` by role. A style has `fontId`, `fontSize`,
  `lineHeight` (px), and optional `fill`; fontId must point to a suitable font record.
  Do not write `fontFamily`, `fontWeight`, `letterSpacing` or `textAlign` yourself: they are currently
  not valid node props. For a different weight use an approved font record.
  Avoid per-node size/lineHeight/fontId overrides because they win over the style;
  if needed, record an exception or create a shared style within the assigned scope.
- **Spacing/radius:** use a dimension token of the right role on gap/padding/radius
  and the allowed properties. Do not turn every geometry number into a token. Do not infer
  an "8px scale" or any other scale if the manifest has not set one.
- **Layout:** row/column for content, free for deliberate overlay/decor. Store
  fixed/fill/hug, min/max, padding/gap instead of x/y computed from screenshots. A root frame
  has numeric width/height; row wrap needs a definite width and a positive minWidth for a direct
  width-fill child. Do not add %, Grid, breakpoints or column wrap on your own.
- **Component:** reuse a master ID via `instance`; only override fields that are
  supported/approved. Do not edit virtual child IDs; update the whole
  `props.overrides` of the instance when needed. Changing a master affects every instance and
  must be within the systemOwner's scope. A name like "Button/Large" does not create a variant axis by itself.
- **State/behavior:** only declare/map the agreed state, navigation and animation.
  `props.states` is not component-set variants. Intent that cannot yet be represented
  goes in a note; do not invent properties or substitute behavior.
- **Missing font/asset:** state the missing ID/hash/status explicitly. Fall back only per the manifest;
  do not silently swap a font or icon and mark it done.

Do not upsert an existing token/style/master just to make your own frame "look better".
The systemOwner's authority here is a coordination convention; the backend does not enforce that permission yet.

## 4. Two AIs working together and revision conflicts

Split frame/node scope clearly, agree on who edits shared resources; prefix IDs per
task to avoid collisions. Do not infer independence from two frames being different:
they may share the same style, alias, font, master or rules note.

`apply_batch` uses `expectedRevision:R`, a key specific to the request body and ordered
operations (tokens before styles before the nodes that use them). Lost response: replay the exact
body and key. If you change the body or expectedRevision: use a new key.

On `REVISION_CONFLICT`, check `changedSince` if present; use `get_changes` to catch up
or re-read when it is missing/truncated/RESET_REQUIRED. Check nodes/ancestors **and**
token alias chains, styles, fonts/assets, component definitions and rules notes that the
batch depends on. If any dependency changed, re-read the context/registry at the new
revision, compare rulesVersion, then rebuild the batch. Only rebase quickly when you have verified that
the changes are independent and have the complete list; always use a new key because the body changed.

If both edit the same semantic decision, keep the decision already settled or hand it to the design
lead; do not let "last write wins" pick a palette/font. Successful validation
only proves the model is valid, not that it follows the design rules.

## 5. Verification and handoff

Re-read authored refs and resolved context at committedRevision; if there is a new write
during verification, read a new consistent pass and state the accepted revision.
Render with the agreed theme/viewport; for a pending job, wait with `render_frame` using
`projectId`, `jobId`, `waitMs` at most 10,000, no dense polling. Inspect with the same
jobId/sceneKey to compare bounds/text lines. Check overflow, text wrap, missing font,
state and viewports per scope; a single screenshot does not prove responsiveness.

Handoff: projectId, rulesVersion, committed/verified revision, theme/viewports,
changed frame/master IDs, token/style/font IDs used or changed, exceptions/gaps and
verification evidence. The recipient must re-read current data; do not use the report in place of the API.
Do not promise full parity with Figma, pixel-perfect on every OS/browser, or policy
enforcement when only a written protocol exists.
