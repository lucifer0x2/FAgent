# Read-only browser viewer

The viewer lives in `web/` and imports the shared Canvas2D renderer from `renderer/`. React and Konva manage the editor shell, local pan/zoom, and selection overlays. By default, WASM computes default-state frame scenes and text lines with the shared Rust core in a browser Worker, using accepted snapshots and exact font bytes; failures fall back to the server. Use `?scene=server` to select server computation explicitly. See [build instructions, measurements and limits](VIEWER-WASM-SCENE.md). Both paths use the existing `paintScene` and `loadResources`; WASM does not paint. A blue outline highlights selection without resize handles.

The Vello engine is the default renderer: a Worker computes and draws the scene on the GPU (WebGPU first) and the main thread keeps a thin overlay; `?renderer=canvas2d` keeps the Canvas2D painter. See [Vello engine](#vello-engine-renderervello).

## Run

```sh
cd web
npm ci
npm run dev
```

The Vite server proxies `/api` to `http://127.0.0.1:3847`. Set `FAGENT_API_URL` to override that target for an isolated test server. `npm run build` creates `web/dist` for the production backend to serve. No demo data is created by starting the viewer.

Project → page → frame selectors use authored snapshots; layers are sorted by numeric storage `sortKey`. Viewport choices are frame size, desktop 1440×900, tablet 768×1024, and mobile 390×844. Frame size uses authored numeric width/height with an explicit 800×600 fallback for nonnumeric values. These choices request computed scenes and never change authored data. The inspector separates authored properties from frame-local computed bounds and records the scene revision.

Drag to pan; wheel, pinch or the zoom buttons to zoom; Fit or `F` to fit. The camera fits once when the selected project/frame/viewport preset first has a ready scene and a nonzero measured pane. Revision refreshes, authored frame dimension changes, panel toggles and window resizing preserve manual pan and zoom. Selecting another project, frame or preset starts a new camera scope; Fit always uses the currently displayed scene and pane dimensions. Wheel zoom is proportional to the wheel delta, so a mouse notch still steps about 1.1x while a trackpad glides in small increments. The frame name and zoom controls float over the canvas, which fills the pane edge to edge. Focus the canvas and use arrow keys to pan and `+`/`-` to zoom. Every layer is a keyboard accessible native button. The header buttons toggle layers and inspector, including at narrow widths; see [Panels and touch](#panels-and-touch) for resizing, narrow layouts and gestures. The canvas remains read only. Font resource import is the explicit mutation exception described below.

## Project fonts

**Fonts** in the header opens an accessible modal for the selected project. It lists
family, weight, style, license and whether a font is stored in the project or bundled
with FAgent. **Download** saves the original font bytes, including bundled
FAgent Noto Sans. Missing files are labeled unavailable.

**Add font** accepts TTF, OTF or WOFF2 files up to 10 MiB. Select the file and enter
its family, weight, style and optional license. Fonts are stored with the project;
there is no OS installation or system-font lookup. Imports do not replace fonts on
existing text nodes. The modal renders metadata in the UI's normal font and does
not claim to preview the selected font.

An interrupted response offers **Retry same import**, retaining the exact request
and idempotency key. A revision conflict refreshes the list and requires an explicit
new **Add font** submission. Switching projects clears the form and cancels requests
for the previous project. Escape closes the modal and returns focus to **Fonts**;
Tab stays within it. The modal scrolls on narrow/short screens.

## Element actions and links

Select an element to reveal **Copy link**, **Export PNG** and **Export SVG** in
the Inspector. Right-click a canvas element or layer to select it and open the
same small action menu. `Shift+F10`/the context-menu key works on layer buttons;
arrow keys move through the menu, Escape closes it and restores focus, and an
outside click dismisses it. Export uses authored frame dimensions and the existing
strict node-export pipeline, independent of viewer zoom/viewport. Pending work,
download completion and errors appear in an accessible status message. SVG keeps
live text rather than outlining it. Export requests never change design revisions.

`Cmd+C`/`Ctrl+C` copies the selected element's link. Native copying in text inputs,
editable content and selected page text is preserved. Clicking empty canvas clears
selection and input focus; a pan or right-click does not deselect. Empty selection
survives snapshot reconciliation and live SSE refreshes.

Navigation lives in the search query: `project`, `page` and `frame` identify the
scope; `node` is optional. A three-key address opens the scope without highlighting
an element. Four-key links also highlight the addressed node, including
percent-encoded virtual instance child IDs. `frame=%3Aall%3A` means All frames;
`page=%3Aall%3A` means [All pages](#all-pages) (Vello engine only).
The URL is the selection authority on load, reload and Back/Forward; legacy
`fagent.selection` storage is no longer read or written. Missing/invalid IDs
are normalized against the authoritative list/snapshot and display fallback notices.
Explicit choices push history; automatic defaults and revision cleanup replace
the current entry. Address-bar changes preserve other query parameters and hash.
Copy link keeps its canonical `/?project=…&page=…&frame=…[&node=…]` contract,
excluding transient viewport/cache/revision parameters. On the All pages board, links and
actions resolve the node's own page and root frame, so a copied link never carries `:all:` as
its page. The MCP connection instructions and
`skill://fagent/implement-frame.md` teach AI clients to confirm the exact node ID
with `find_nodes`, then read its `get_design_context`; deleted/moved IDs must be
reported rather than guessed. A stale browser address displays a fallback notice.

## Panels and touch

### Layers panel

The Layers list is virtualized (`web/src/components/Layers.tsx`, `web/src/components/layers/`).
The visible tree (collapsed nodes and closed pages skipped) is flattened into rows of a fixed
33 px (`ROW_HEIGHT`, matching `.layer-row` and `.layer-group-heading` in `workspace.css`); only
the rows inside the panel's scroll viewport plus 12 rows of overscan on each side are mounted,
each placed absolutely at its index × row height, and the panel turns off scroll anchoring. The
row holding keyboard focus stays mounted when it scrolls out of the window. Expanded/collapsed
state lives above the rows (`useLayerExpansion`), so it survives rows leaving the window: node
rows start expanded, and a new selection reopens its collapsed ancestors.

A new selection, a revision or the panel shown again scrolls the selected row to the centre of
the panel unless it is already fully visible; expanding or collapsing other rows afterwards
leaves the scroll alone. A virtual instance child has no row of its own, so its instance row is
revealed instead (the deepest selection ancestor with a row).

On the [All pages](#all-pages) board, frames are listed under one collapsible heading per page
(page accent dot, frame count). Pages start collapsed; the page holding the selection opens,
also after the user closed it by hand, and can be closed again.

### Resizing and narrow layouts

On layouts wider than 800 px, the inner edge of Layers and of the Inspector is a resize handle
(`PanelResizer`, an ARIA `separator`): drag it, use the arrow keys (16 px steps) or Home/End
(minimum/maximum), and double-click to restore the stylesheet's responsive default. Widths are
clamped to 200–500 px, each panel is also capped so the canvas keeps room, and the chosen width
is remembered per panel in `localStorage` (`left-panel-width`, `right-panel-width`; unreadable or
invalid values mean the default). A drag sizes the panel element directly and commits once on
release.

At 800 px and narrower the panels are overlays without resize handles (`usePanelVisibility`):
both start closed so the canvas gets touch input, only one is open at a time (opening one closes
the other), and the choice is kept in memory only. Wide layouts keep the remembered `left-open`/
`right-open` choice (default open). Crossing the breakpoint re-applies these defaults.

### Touch gestures

`useCanvasGestures` handles wheel zoom and multi-touch on the canvas host for both the Vello
engine view and the Canvas2D/Konva view; its pointer bookkeeping (`PointerGesture`) and the pan
step live in `hooks/canvasPointers.ts`. One pointer pans and taps: both views pan on the host
(Konva's stage drag is not used) with pointer capture, applying each move as a delta to the
latest camera (`panBy`). Wheel and ctrl+wheel (trackpad pinch) zoom about the cursor as a step on
the latest camera too, so a dense smooth-scroll stream (e.g. Mac Mouse Fix, momentum included)
composes with a drag in progress: the point grabbed stays under the cursor and nothing jumps.
A second pointer stops that pan: two fingers pinch-zoom about their midpoint and pan as the
midpoint moves, starting from where the fingers are when the second one lands; lifting one finger
keeps panning with the other. The pointer-up that ends a multi-pointer gesture is never a tap, so
a two-finger tap does not select or deselect. Pointer cancel and the host losing pointer capture
count as an up, and a new first pointer ends a gesture whose up was missed. On iOS Safari,
`gesturestart`/`gesturechange`/`gestureend` are prevented so the page itself does not pinch-zoom
over the canvas.

## Zoom rendering

The canvas paints the scene as vectors on every draw: a Konva `Shape` whose `sceneFunc` calls the
shared `paintScene` under the stage transform (zoom times `devicePixelRatio`). Text and edges are
therefore rasterized at the size they are shown, during a wheel or trackpad gesture as well as at
rest, up to the 4x maximum zoom, and the picture does not change once a gesture stops. Nothing is
re-rasterized or cached per zoom level: the Konva layer stays the size of the canvas pane, so a
zoom allocates no canvas and costs one paint of the scene per frame (about 1.4 ms p95 for an
857-node, 716-line scene in headless Chromium). Images are drawn with
`imageSmoothingQuality = "high"`, which only affects the viewer, never the worker PNG.
This describes the single-frame view. The All board paints the same vectors during a zoom
gesture, then caches each frame as a bitmap once the zoom rests
([VIEWER-FRAME-PAINT.md](VIEWER-FRAME-PAINT.md)).

`paintScene` is the same code the worker PNG runs through `drawScene`, which only validates the
pipeline, sizes and clears the target and sets its scale before painting. The viewer still draws
the scene once at 1:1 with `drawScene` when a job arrives: that offscreen canvas checks the
pipeline and stays byte-identical to the worker PNG, which is what the acceptance run compares.
Each node paints fill, then image or text; a node's `stroke` paints after its whole subtree
(`renderer/stroke.ts`), under ancestor clips and opacity but not its own clip, so an outside
stroke of a clipping frame stays visible.
`npm --prefix web run test:zoom` measures sharpness against a `drawScene` reference mid-gesture
and at rest, canvas allocation, and frame cost on 800×600, 1440×900 and 2880×1800 fixtures.

## Opacity

`opacity` composites like CSS (`renderer/layers.ts`, `paintTree`): the scene is walked as a tree,
and a node below opacity 1 whose subtree issues two or more paints renders into an offscreen layer
at alpha 1, which is then drawn once at the accumulated opacity, so overlapping children do not
show through each other. A node's fill and its stroke count as two paints, so a faded stroked box
fades as one image like a CSS border. A single paint (a leaf, or a group with one painting child) folds the
alpha instead, which is identical and keeps such scenes byte-identical to the old per-node alpha.
A layer covers the device-pixel box of what its subtree paints (`renderer/scene-tree.ts`; text is
padded for glyph overhang; outside/center strokes widen it past bounds, and a node's own clip never
cuts its own stroke), clipped to the target and to clipping ancestors, offset only by whole
pixels so antialiasing matches direct painting. Layers are `HTMLCanvasElement`s: Chromium's
`OffscreenCanvas` clips without antialiasing. Live layers share a 16M-pixel budget; a group over it
folds its alpha like before and counts in `opacityLayerFallbacks` of `drawScene`'s result, next to
`opacityLayers`. The viewer allocates its layers on every draw (no pool): a 2880×1800 scene with
56 opacity groups measured about 3.4 ms p95 per zoom frame against 1.6 ms without them.

## Consistency

### All frames on a page

**All** is the default for initial, invalid, project and page selections. A valid saved
single-frame selection or element deep link remains respected. All shows every root
frame of the page on a shared pan/zoom canvas and persists across reload and SSE. Packing
(`fitFrameBoard`, `web/src/scene/frameLayout.ts`) uses actual frame dimensions and authored
order only: each frame, with a label band above it, goes to the highest free spot of a skyline
(`web/src/scene/skyline.ts`), then the leftmost, so short frames stack in columns beside a
tall one instead of starting a shelf row under it. Frames never fill holes under the skyline,
and a later frame never starts both above and left of an earlier one, so the order reads left
to right, top to bottom. The target width comes from a bounded search (40 log-spaced widths
between the widest frame and one long row) scoring the Fit envelope at a fixed 16:10 reference
view (`REFERENCE_VIEW`, 800×500 CSS px after the Fit margins: the pane of a 1440×900 window with
both drawers open) plus the wasted area. Label room is sized so labels read at that reference
Fit (28 px), which also holds on wider and taller panes. Labels stay 12 CSS pixels
at Fit and normal zoom, with ellipsis bounded to the displayed frame width. At extreme
zoom-out, collision-prone labels hide; names remain in Layers. Neither zoom/pan nor the pane
size (window resize, Layers or Inspector drawer) repacks: they only move the camera, which
keeps its scale and offset from the pane's top-left corner, as for a single frame (Fit or `F`
re-fits). About 10 ms for 200 frames (Node, Apple Silicon), computed once per change of frame sizes.

The whole board appears immediately with static skeleton slots. WASM computes one
frame at a time, yields for paint/input between frames and pauses new admissions while
the tab is hidden. Each frame replaces its skeleton as soon as its own resources are
ready; accessible `x/y` progress stays compact and the camera/geometry stays stable.
Production builds share a bounded IndexedDB scene cache between All and single-frame
views, always after an accepted authoritative snapshot, keyed by each frame's content
digest so unchanged frames are reused across revisions.
See [cache identity, bounds and verification](VIEWER-PROGRESSIVE-CACHE.md).
The board shares loaded manifests. Each ready frame keeps one bitmap at the resting zoom,
never larger than the canvas, so board redraws blit instead of repainting every frame; see
[VIEWER-FRAME-PAINT.md](VIEWER-FRAME-PAINT.md).
Server mode and fallback admit batches of four, waiting for confirmed terminal jobs
before advancing; unknown timeout/transport errors stop admission and expose Retry.
Failed frames retain labeled slots, and fallback retains completed browser frames.
Selection, actions, links and inspector stay frame-local; healthy current frames remain
inspectable during progressive loading and partial failure. Board revision changes keep
older scenes on screen until each frame revalidates (unchanged frames immediately); they
never expose old geometry as current to hit testing or the inspector.

See [ALL-FRAMES-REPORT.md](ALL-FRAMES-REPORT.md) for checks and the macOS Chromium
launch limitation. Unit regressions run in `npm --prefix web test`; the isolated
browser fixture is `npm --prefix web run test:all-frames`.

Selection persists in the URL query. Reload validates the project, page, frame, and optional node against the current snapshot, falling back to All when a frame disappears. A visible message explains removed page/frame/layer fallbacks.

The first snapshot request and event stream start together. SWR owns project-list and
snapshot server state inside the authorized viewer lifetime. Snapshot request keys include
project, known revision watermark, object representation, visibility scope and an explicit
metadata invalidation epoch; scene viewport/theme/frame parameters are independent. The
snapshot endpoint returns latest, so a newer response is stored only under its actual
canonical revision. Same-revision events, list refreshes, focus/reconnect and warm project
switches reuse that payload without another snapshot GET. Every stream open reconciles the
authoritative list/revision watermarks, and durable commits replay from the last accepted
HTTP snapshot cursor. Foreign/duplicate commits are ignored; newer commits advance the
watermark. Different revision requests share an in-flight latest read, then perform a
follow-up if the response did not satisfy a newer queued watermark. Explicit reset,
visibility and manual retry invalidate metadata even at the same authored revision, retaining
ETag/304 and monotonic snapshot checks. Reconnect uses bounded exponential backoff and jitter.

Each render request has an effect lifetime and validates project/revision/frame, expected job ID, viewport, theme/state, and source scene key. A newer revision in the same project/frame/preset retains the last ready scene and loaded resources while pending or failed. A compact, nonblocking status explicitly names the displayed scene revision and requested revision, with Retry on failure; the canvas remains mounted and interactive even during an active drag. The region exposes `aria-busy` while pending. Computed inspector geometry is suppressed until the scene exactly matches the current request, so it cannot silently mix with newer authored properties. Project/frame/preset navigation immediately hides incompatible content before effects run. Late resources and jobs cannot publish into a newer request. Pending jobs are polled at 500 ms and time out after 45 seconds. The top-level job error is distinct from snapshot/connection error; missing fonts and assets display labeled shared-renderer placeholders and explicit status text. Scene and authored revisions are displayed separately.

`npm --prefix web run test:continuity` covers rapid revisions, retained Stage/canvas identity,
zoom and active drag during refresh, failed/retried renders, mismatched and obsolete jobs,
authored dimension changes, panel/window resizing, explicit Fit/F, and deliberate scope changes.
Its in-memory fixture and Vite server use random loopback ports and are cleaned up after the run.

### All pages

With the Vello engine the Page selector lists **All** first (described by its page count).
`page=%3Aall%3A` (`ALL_PAGES` in `web/src/frameScope.ts`, never sent to the server as a page ID)
shows every root frame of every page on one board, in page order; the frame scope is always All
frames. Canvas2D (chosen, server scenes, or the engine fallen back) has no All pages board
(`useAllPagesBoard` in `web/src/engine/useEngine.ts`): the option is not offered and an All pages
address moves to the first page with All frames (replacing the history entry).

All pages is the default scope: an address without `page` (fresh open, project switch, public
viewer) whose `frame`/`node` names no page opens the board and writes `page=%3Aall%3A&frame=%3Aall%3A`
back with replaceState, so a reload shows the same view (`reconcileSelection` in `web/src/state.ts`).

**Layout** (`fitPageBoard`, `web/src/scene/pageBoard.ts`). Each page's frames are packed as on a
single page board, with padding and label room scaled by that page's own fit unit (board units
per screen pixel when the page alone is fitted into the reference view), so any page zoomed to
fit shows about the same padding and label size. The pages are then packed as blocks with the
same skyline packer and width search, so small pages stack beside a tall page instead of in a
row under it; blocks are spaced by the mean fit unit, with room above each block for its page
title sized to read at the whole board's Fit in the reference view. As with `fitFrameBoard`,
positions depend only on the content (frame sizes, pages, authored order), never on the pane
size, the drawers or zoom. About 7 ms for 200 frames over 30 pages.
Each page also has a focus rectangle: its area plus the title room above it.

**Sections** (`web/src/engine/sections.ts`). Page backdrops (light tint and a 1 px square outline
in the page's accent colour, cycled per page) are painted on the `.engine-underlay` canvas under
the engine image, which clears to transparent. Title tabs (page name and frame count, 13 CSS px)
are painted on the overlay above each page, never over its content; when the page's top is
scrolled off, the tab stays pinned to the top of the canvas. A crowded tab drops the count, then
ellipsizes the name up to the next tab, and shrinks to an accent dot when no name fits. Clicking
a tab zooms to that page (`fitTo` its focus rectangle).

**Selection.** The Frame selector lists every frame with its page name as description; choosing
one opens that frame on its own page. Reconciliation keeps a selected node of any page while the
board is shown. Layers groups frames under their pages ([Layers panel](#layers-panel)), and
links and actions resolve the node's real page and frame. The canvas title reads
`All pages · <pages> · <frames> frames`.

## Live workspace

The viewer keeps **exactly one `EventSource` per tab**. The server speaks HTTP/1.1 without TLS, so
a browser allows about six connections per origin; two streams per tab would starve snapshot, job
and asset requests after three tabs. With a project selected the tab uses the project stream
(`/api/v1/projects/{id}/events`); with none (an empty workspace) it uses the workspace stream
(`/api/v1/events`). Switching closes the previous stream before opening the next, and a failed
stream is closed before its backoff timer opens a replacement.

No stream opens until `GET /api/v1/viewer` answers. When it reports `liveUpdates:false` (a public
listener with live updates off, see [SERVER.md](SERVER.md#public-live-updates)) the tab opens no
`EventSource` at all: `useProjectLive` reports connection `static`, the status bar shows
"Reload for latest" instead of Live/Reconnecting and hides "Follow agent", the project list does
not revalidate on focus or reconnect, and the public preview keeps the snapshot it loaded. A
reload reads the newest revision.

- **Live project list.** `project_created` refreshes the authoritative HTTP list. All pages
  are read before publishing membership. While the workspace is empty the viewer selects
  the first listed project and switches to its project stream; otherwise a short notice
  names the new project. A hint alone cannot add a project to the selector.
- **Activity marks.** `project_activity` from another project bumps its revision in the list,
  marks it with `●` in the selector, shows a count next to the selector and a short notice with
  the project's name. It never refetches the snapshot in view. Selecting the project clears its
  mark and loads its newest revision.
- **Hints are not durable.** Every stream open/reconnect and reset refreshes
  `GET /api/v1/projects`, including pagination. HTTP owns visible membership; hints retain
  newer revision watermarks for listed projects. Unchanged watermarks reuse snapshots;
  reset/visibility events explicitly revalidate snapshot metadata. Removed projects and
  snapshot 404s invalidate their cached payloads.
- **Follow agent** (status bar checkbox). When on, `project_created` or `project_activity` moves
  the viewer to that project. It is **off by default** (`FOLLOW_AGENT_DEFAULT` in
  `web/src/hooks/useFollowAgent.ts`), because jumping away from the project in view loses
  context, and the choice persists in local storage (`fagent.followAgent`) when available.

Code: App explicitly composes `useProjectList` (SWR list and revision hints),
`useProject` (URL-derived selection/fallback notices only), `useSnapshot` (SWR snapshot), and
`useProjectLive` (single-stream handlers, follow/activity). `useProjectStream` owns connection
lifetime/backoff; `useFollowAgent` owns the follow preference. Pure reducers
`mergeProjects`, `bumpRevision`, `markActivity`, `clearActivity`, `pickProject` and
`followTarget` live in `web/src/state.ts` with unit tests. Passive scene viewing sends only GET;
explicit font imports and computed exports use their documented POST endpoints.

The scoped cache retains at most eight latest project snapshot payloads, with no historical
trees. Revisiting an evicted project reads it again. Small request/pointer/epoch records
remain until that project is evicted or the authorized route unmounts. Locking/unmounting
discards the whole provider and aborts its pending snapshot reads; another authorization
lifetime cannot reuse its payloads or validators.

`useNavigationUrl` observes history through `useSyncExternalStore`; selection is
never copied into a React reducer or localStorage. Pure selection transitions are
applied to the address, with obsolete-address checks before automatic writes.
Follow hints for unlisted projects wait for authoritative membership and cancel
when the user navigates, disables follow, retries, disconnects or unmounts.
See `docs/URL-NAVIGATION.md` for verification and the navigation contract.

## Vello engine (`renderer=vello`)

Default renderer since 2026-11-17 (built in Task 30, P1 of [VELLO-GPU-RENDERER-RESEARCH.md](VELLO-GPU-RENDERER-RESEARCH.md)).
Canvas2D/Konva remains selectable and is the fallback; this path never removes it.

**Selecting it.** With no choice the viewer starts the engine at WebGPU (`?renderer=vello` is the
same). `?renderer=canvas2d` (or `localStorage["fagent.renderer"] = "canvas2d"`) keeps Canvas2D,
and so do server scenes (`?scene=server` or `fagent.scene`), which only Canvas2D consumes; an
explicit renderer wins over the scene choice and the query wins over storage. `?renderer=webgl2`
and `?renderer=cpu` start further down the chain (testing). Browser tests of the Canvas2D painter
pin it with `web/tests/canvas2d.mjs`. `?glyphAtlas=1` switches GPU text from outlines
to the experimental `vello_gpu` glyph atlas (outlines stay the default and the fallback). The status
bar shows the backend (`Vello · WebGPU`, `Vello · WebGL2`, `Vello · vello_cpu`).

**Architecture.** One dedicated Worker (`web/src/engine/engine.worker.ts`) owns one module,
`crates/engine-wasm` (scene-wasm compute + paint-vello encoder + `vello_gpu`/`vello_cpu`):

- The page sends the accepted snapshot once per revision (`sync`), the frame targets, the board
  layout and the camera. The worker parses the snapshot once, computes frames one at a time
  (fonts fetched and hash-checked in the worker, images decoded with `createImageBitmap`), keeps
  every scene in Rust and draws on its own `OffscreenCanvas`, at most one draw per worker
  animation frame. Between two frame computations the worker yields through a `MessageChannel`
  (`web/src/engine/yieldTask.ts`; a nested `setTimeout(0)` chain is clamped to 4 ms per frame),
  so camera, layout and sync messages received meanwhile run first, and a camera or layout
  change is drawn before the next frame starts (also after a frame fetched its fonts, since
  messages ran during the fetch). Waits on the GPU (backend attach, renderer and
  present probes) poll on a timer instead (`pollTask`), so they do not spin the worker. Committed frames are drawn progressively but
  paced (`web/src/engine/drawPacing.ts`): every draw encodes the whole board, so the first frame
  of a sync is drawn at once and later ones together at most once per gap (10× the last draw
  time, 100–250 ms); the last ones are drawn right before `synced`, so they never wait for an
  animation frame (hidden or non-composited page). A committed frame's `frame` message is held
  until the image that shows it and posted just before that `drawn`, so the page never drops a
  skeleton before the pixels; camera and layout draws in between show held frames without
  releasing them (releasing would change hug heights, hence the layout, one draw per frame
  again). If that last draw fails (no backend, a GPU error), the held frames and `synced` wait
  for the first image the rebuilt or next backend draws, then go out in the same order (frames,
  `drawn`, `synced`); a newer sync drops the pending `synced`. Failed and reused frames are
  reported at once, except a reused frame whose message from an older sync still waits for its
  image (that draw failed): it waits with it. Unchanged frames (same content digest) move
  to the new revision without recomputing and repeat the summary and missing assets the engine
  shows, so a view that never received them (another scope showed the frame first, or a newer
  sync replaced the view) replaces that frame's skeleton too. A frame stays drawn until its
  replacement is committed; a commit the engine refuses (the frame was released meanwhile) is
  reported as that frame's error, never as shown. Frames are computed only once the backend is
  attached (its async setup would otherwise wait one whole frame computation per step).
- Each draw goes to the page as an `ImageBitmap` (`drawn`, transferred) with the layout and camera
  it was drawn with. The page shows it on a `bitmaprenderer` canvas and, in the same task, places
  it for the current camera with a CSS transform (`web/src/engine/staleCamera.ts`). A frame
  computation (~0.75 s for a large frame, synchronous WASM) keeps the worker from redrawing; during
  it a pan or zoom moves and scales the last image with the overlay, and the next draw replaces it
  in the same main-thread commit as its placement. A canvas transferred to the worker would reach
  the screen on the compositor's own schedule, out of step with the overlay.
- **Overscan.** Off by default. The server environment variable `FAGENT_VIEWER_OVERSCAN` (percent
  per side, integer or decimal 0–50, default 0; an invalid value stops startup) sets how far the
  worker draws beyond each viewport edge; for example `FAGENT_VIEWER_OVERSCAN=10` draws 10%. Both
  listeners report it as `overscan` (ratio, `0.1`) in `GET /api/v1/viewer`; `useEngineStart` reads
  it (`useViewerOverscan`, 0 while pending, after a failed probe or from an older server) and
  `engineClient.setOverscan` sends it to the worker as an `overscan` message, before or after the
  worker starts. With ratio 0 the canvas is exactly the viewport and stale placement is unchanged.
  With a ratio r (`web/src/engine/overscan.ts`; margin = round(r × side) device px per axis) the
  worker draws a canvas of viewport + 2·margin with the camera offset by the margin, and `drawn`
  carries the margin. The page places
  that image at −margin (`stalePlacement`), so on-screen pixels match the camera exactly, and while a
  new image is pending a pan of up to the margin slides in drawn pixels instead of a blank strip. The
  margin shrinks (never the viewport) so the canvas fits the backend's max texture size (WebGPU device
  `maxTextureDimension2D`, WebGL2 `MAX_TEXTURE_SIZE` capped by `MAX_RENDERBUFFER_SIZE`, 4096 when
  unknown, e.g. vello_cpu); `fagentEngine.status().overscan` reports `{ratio, x, y, maxTexture}`. The
  vello host uses `overflow: clip`. Hit tests, outlines, labels, Inspector bounds and `fitTo` use the
  page camera, unchanged. Cost of 10% on `engine-board` (WebGPU, DPR 2, A/B in one session): no measurable
  change at Fit (all 12 frames already in view; run-to-run noise ±5 ms), ~100% pan p50/p95 4.3–6.2 /
  4.9–8.6 → 8.5–11.9 / 10.9–14.0 ms (253 instead of 177 nodes encoded, 1.44× pixels).
- Scenes never come back. The page receives a small summary per frame (revision, size,
  diagnostics, missing assets, renderer version, hit area) and asks the engine for hit tests and
  node bounds (`hit_test`, `bounds`, `frame_status`). Clicks outside every frame's hit area are
  background immediately, as on the Konva canvas.
- The main thread draws only a thin Canvas2D overlay (`web/src/engine/overlay.ts`): frame labels
  (12 CSS px, screen space), skeleton and error slots, hover and selection outlines, and on the
  [All pages](#all-pages) board the page title tabs (page backdrops go on a Canvas2D underlay
  below the engine image). Selection changes send nothing to the GPU.
- Board drawing (`crates/engine-wasm/src/draw.rs`): per placed frame the white box with the Konva
  drop shadow, then the scene through the shared encoder. Always on: WASM `simd128`, viewport
  culling (whole frames and subtrees). Level of detail from Task 29: text below 4 device px is
  drawn as bars; shadows use the analytic blurred rect below device scale 0.5 and the filter layer
  above.

**Engine scene cache (Task 42).** The worker keeps computed frames on disk, so reopening a
project whose frames are unchanged imports them instead of running `frame_keys` and `compute`
(measurements below):

- **Store.** IndexedDB `fagent.engine-scenes`, opened in the worker and separate from the
  Canvas2D `fagent.browser-scenes` (`web/src/engine/recordStore.ts`; policy in `sceneCache.ts`).
  `records` are keyed by `[build, projectId, digest, width, height, theme]`, never a revision:
  the record bytes, the fonts `[{id, hash}]` and assets they draw with, and their size. `usage`
  holds the same keys with size and last use, so eviction never loads records; `manifests` map
  `[build, projectId, etag, framesKey]` (`framesKey` = SHA-256 of the theme and frame list) to a
  digest per frame id. `build` is the module's content hash (its manifest file name) plus the
  record format; a module without a hash keeps nothing. A worker's first committed write deletes
  every row of other builds, once per worker: a tab left open on another build (across a deploy)
  does not wipe this build's rows on each of its loads, nor this one its rows.
- **Records.** `export_record` writes a committed frame (`crates/engine-wasm/src/record.rs`,
  format 1): JSON of the scene, its structured diagnostics, the request viewport and digest,
  serialized from the frame's own paint scene (no second copy in memory). Every `f32` reads back
  bit for bit (digits that would round differently through `f64` are written as their exact
  `f64` widening; `float_roundtrip` parses `f64` exactly), so an imported frame paints,
  hit-tests and measures like the computed one. `import_record` stages exactly what `compute`
  stages, restamped to the accepted revision like `reuse` (its `sourceSceneKey` names the
  computing revision), and refuses a record whose format, project, frame, viewport or digest
  differs, one that repeats a node id, or one whose fonts are not all loaded (an error, never a
  frame without text).
- **Bounds.** Records measured 61 KB on average on the 229-frame fixture (14.0 MB) and 62 KB on
  the 904-frame one (56.2 MB): 2,000 records / 256 MiB (two 1,000-frame boards of such frames, e.g. before and
  after a shared token edit), 16 MiB per record (larger ones are not stored; counted as
  `oversized`), least recently used first on write (a load's imports and manifest hits refresh
  their last use, and records touched in a batch rank as used now before that batch evicts, so a
  load never evicts what it has just imported); 32 manifests. The Canvas2D cache's 256 entries
  would not hold one large board.
- **Load.** `sync` carries the strong ETag the server sent with the accepted snapshot
  (`useSnapshotEtag`). A new ETag at the same revision (a 200 with new bytes) sends the snapshot
  again, and the worker trusts an ETag only when it arrived with the snapshot it loaded, so
  digests are never filed under another representation's ETag. The worker reads the manifest
  for that ETag and frame list: a hit gives the digests without `frame_keys`; a miss runs
  `frame_keys` and stores them; a sync superseded during that read stops before `frame_keys`
  and stores nothing. Then one readonly
  transaction asks for every frame the engine does not already show with its digest; each
  record resolves on its own, and the read starts before the first frame, so it overlaps the
  backend attach. Per frame: in-memory `reuse` first, then a stored record (its fonts through the
  hash-checked path, `import_record`, images as for a computed frame, commit), else `compute`.
  Imports wait for the backend like computations: on a warm reload the attach ends ~60 ms after
  `sync` (no `frame_keys` blocks it any more), so starting imports earlier could save at most
  that. Restored frames never trigger the first-frame draw of a sync (`drawPacing.ts`): they are
  shown once the progress gap has passed since the first of them, or right before `synced`;
  the first computed frame of the sync is still drawn at once after restored ones were shown.
- **Writes.** After `synced` (after the draw that shows the frames): one `export_record` per
  yield, never while a load runs, in batches of at most 64 records / 32 MiB per transaction,
  with the manifest and the last use of imported records.
- **Failures.** Opening has a 1 s deadline, a read 3 s, a write 10 s; a timeout, quota error or
  blocked database reads as a miss or skips the write. A record the engine refuses (damaged,
  another format) is deleted and its frame computed and written again; a damaged font list
  computes too, and a damaged manifest (digests not an object of strings) is a miss. A font a
  record lists but whose fetch fails is not remembered as failed (computing asks which fonts
  the frame needs and reports those); neither is a failure of a sync a newer one replaced. Retry
  (`bypass`) reads neither manifest nor records and replaces what it
  recomputes; a newer sync aborts the older one's read. The cache never fails a frame.
  `fagentEngine.status().cache` reports hits, misses, bytes and time read, imports, rejections,
  writes and pending writes.
- **Access.** The cache never decides access: the worker reads it only for a sync that carries a
  snapshot the server answered in this session (digests from that snapshot or its ETag). It holds
  computed scene records, digest manifests and verified font bytes; no credentials, tokens or
  snapshot JSON. `navigator.storage.persist()` is called where the worker has it (browsers expose
  it to windows only, so no permission prompt is raised).
- **Fonts.** Fonts are served without caching headers, so every load fetched and hash-checked
  them again, mostly before the first import: an 86 ms font window in a 631 ms warm `synced`
  (14%, above the 5% threshold of Task 42 item 5) before this cache, 0 after it. The worker
  keeps verified font bytes in the Cache API (`fagent.engine-fonts`, `web/src/engine/fontCache.ts`),
  keyed by content hash under a synthetic same-origin URL, at most 64 fonts and 128 MiB (oldest
  stored first; sizes kept in a response header, so eviction reads no font body; a font over
  128 MiB is not kept); a hit is hash-checked again and a damaged entry fetched anew. A failed
  `caches.open()` is retried by the next load; a put over the origin's quota frees at least
  that font's size from the oldest entries and is tried once more (the fetched bytes are used
  either way).

**Fallback.** WebGPU (adapter + device + Vello renderer probe) → WebGL2 (`failIfMajorPerformanceCaveat`,
probe) → `vello_cpu` into an `ImageBitmap` (`transferFromImageBitmap`) → Canvas2D for the rest of
the session. A canvas keeps its first context type, so each step draws on a fresh worker canvas
(and the page mounts a fresh display canvas). A lost
WebGPU device is rebuilt once on the same canvas; a second loss moves to WebGL2. WebGPU also leaves for WebGL2
(for the session) when it fails without losing the device: a one-time present probe at attach
(`web/src/engine/presentProbe.ts`: two 16 px frames of known opaque colours through the real
canvas path, `getCurrentTexture` → submit → `transferToImageBitmap`, read back in the worker; ~13 ms
once in headless Chromium, nothing per frame) catches a canvas that stays blank, keeps a stale
frame or swaps channels, which Vello's offscreen renderer probe cannot see; uncaptured GPU errors
(validation, out of memory, internal; wgpu's web backend installs no handler, so they would only
reach the console) are captured in Rust and checked before each render and every second; a
render/present that throws fails over too. A Rust panic aborts the module: the page moves to
Canvas2D with "Engine module aborted: …" (the panic text is logged as `fagent engine panic: …`).
Each fallback is also logged as `[fagent engine] …` in the console. Debug hooks for these paths:
`?engineDebug=1&gpuFault=blank|throw|error` (at attach) and `fagentEngine.gpuFault(…)` (later). Every step is
recorded (`window.fagentEngine.state().notes`, the view's diagnostics); a module that cannot load
shows "Vello renderer unavailable; using Canvas2D: …" and never leaves a blank canvas.
`window.fagentEngine` (status, state, camera, selection, `loseDevice` test hook) exists only in
this mode.

**Build.** `npm run build:engine-wasm` (needs `wasm32-unknown-unknown` and wasm-bindgen 0.2.129, as
for scene-wasm) builds the speed profile (O3, thin LTO, `+simd128`, `panic=abort`) and writes
`web/public/engine-wasm/fagent_engine_wasm[_bg]-<content hash>.{js,wasm}` with `.br`/`.gz` and a
`manifest.json` the worker reads with `no-store`; `vite build` copies them to `web/dist`. The
server marks the hashed `.js`/`.wasm` (and their `.br`/`.gz`) `immutable` for a year while the
manifest stays `no-store` ([cache policy](VIEWER-CACHE-POLICY.md)). Docker
builds it in the `engine-wasm` stage. Without the module, `renderer=vello` falls back to Canvas2D.

### Measurements — 2026-11-17

Apple M3 Pro, Chromium 156 (full build, new headless, real GPU), production build.

- **Module:** `.wasm` 6.00 MB raw / 1.83 MB gzip / **1.25 MB brotli**; glue JS 104 KB / 15 KB
  brotli. In this mode it replaces scene-wasm (3.77 MB / 0.70 MB brotli), so the extra transfer is
  ~0.55 MB brotli (vello_gpu + wgpu WebGPU/WebGL + vello_cpu).
- **Board** (`web/tests/engine-board.mjs`: 12 frames, 4 945 nodes, 1440×900 at DPR 2, WebGPU),
  worker-side encode + submit per full redraw: Fit (device scale 0.15, text and shadows at LOD)
  **11.8–12.0 + 2.3 ms**, draws while panning at Fit p50/p95 **13.3/13.8 ms**, while zooming
  11–12 ms; ~100% (2 frames in view, 623–3 026 glyphs) encode + submit 1.6 + 0.6 to 5.5 + 1.1 ms,
  pan p50/p95 2.1/2.8 to 5.6/6.4 ms. Glyph atlas (`?glyphAtlas=1`) changes nothing at Fit (text is
  at LOD) and cuts the text-heavy 100% view to 3.1 + 1.8 ms (pan 3.3/4.0 ms). Module fetch +
  instantiate 23–28 ms (warm cache), WebGPU init 44 ms (136 ms on the first launch). Time to
  "Scene rN" for the whole board: 1.8–1.9 s (Canvas2D 2.8–3.6 s on the same run).
- **Main thread:** no long task over 50 ms while panning or zooming, and none while loading with
  the Layers panel closed. With Layers open, one ~90 ms task remained on load for both renderers
  (Canvas2D 91–96 ms): React commit and style recalculation of 4 945 layer rows (CPU profile), not
  the canvas path. The Layers list has since been virtualized ([Layers panel](#layers-panel)),
  which mounts only the rows in view; this task has not been re-measured.
- **Pixels:** the engine module on `OffscreenCanvas` reproduces the Task 29 captures on all 77
  parity cases (`node spikes/vello-gpu/parity.mjs --gpu --engine`): same verdicts (WebGPU/WebGL2
  40 pass / 29 fail / 8 text review, vello_cpu 39/30/8), max channel difference from Task 29 = 1
  on WebGPU/WebGL2. In the viewer, `engine-smoke` differs from Canvas2D in 0.58% of pixels of the
  fixture screenshot (text antialiasing).

### Measurements — 2026-11-26 (large-board cold load)

Apple M3 Pro (quiet: load average 2.3–2.9 on 11 cores), Google Chrome 158 (new headless, real
GPU, WebGPU), production build, 1440×900 at DPR 2, Layers closed. Fixture
(`web/tests/engine-load-fixture.mjs`): 225 screens over 10 pages + 4 component masters = 229 root
frames, 6,782 authored nodes, 3,602 instances, 5 fonts, root frames `height: "hug"`. Base is
`77e2430` (viewer and engine wasm both built from it); new adds paced draws, `MessageChannel`
yields and the shape plan cache. Interleaved: five rounds of one cold run per build (base, new,
base, …), each run a new browser context; the same release server for both. p50 (min–max), ms
from navigation:

| | base All pages | new All pages | base `pg-1` (23 frames) | new `pg-1` |
| --- | --- | --- | --- | --- |
| Worker created | 362 (360–365) | 359 (355–362) | 361 (355–369) | 363 (361–366) |
| `sync` sent | 416 (414–418) | 410 (408–415) | 393 (383–399) | 394 (389–396) |
| `backend` (WebGPU attached) | 946 (943–961) | 944 (938–945) | 837 (823–844) | 836 (827–847) |
| First frame committed | 983 (979–996) | 980 (975–983) | 969 (959–978) | 967 (958–976) |
| First `drawn` with it | 984 (980–997) | 980 (976–983) | 976 (966–985) | 967 (958–976) |
| **`synced`** | **8,595 (8,534–8,777)** | **2,824 (2,812–2,829)** | **1,739 (1,721–1,754)** | **1,199 (1,192–1,210)** |
| Last `drawn` | 8,610 (8,550–8,793) | 2,844 (2,832–2,849) | 1,755 (1,736–1,770) | 1,217 (1,210–1,228) |
| Draws while loading | 413 (408–419) | 33 (32–34) | 39 (37–39) | 7 |
| Worker `render` total | 4,805 | 393 | 528 | 110 |
| Worker `compute` total | 2,526 | 1,305 | 293 | 165 |
| Worker `frame_keys` | 466 | 466 | 362 | 365 |
| Worker `required_fonts` | 99 | 80 | 12 | 11 |
| Worker `set_layout` (calls) | 31 (227) | 4 (20) | 1 (24) | <1 |
| Main-thread long tasks | none | none | none | none |

- `synced` on All pages drops by 5.8 s (−67%), on one page by 0.54 s (−31%); every run of the
  new build is faster than every run of the base. `load_font` (11 ms) and `load_snapshot`
  (10 ms) are unchanged; the remaining cost is `compute` (1.3 s: shaping, allocation, JSON,
  sha256 digests) and `frame_keys` (0.47 s, whole-snapshot resolve; Task 35).
- Progressive display: the new build draws every ~56 ms on average while loading ((`synced` −
  first frame) / draws). The 250 ms bound caps the pacing gap, not the interval between two
  draws: pacing is checked between frame computations, so one computation longer than the gap
  delays the next draw by its own length. The harness then timed
  "commit → drawn" from the page's `frame` message, which is ~0 on the new build because that
  message is posted just before the `drawn` that shows it (base: p95 18 ms, one draw per frame);
  since 2026-11-27 it measures from the worker's commit (below).
- Zooming while loading (`--interact`, 3 runs each, 6 cameras per run): every camera is drawn
  before the next frame; camera → `drawn` p50 14–20 ms, max 25–33 ms (base 11–18 / 19–22 ms);
  `synced` 2.86–2.89 s (base 8.51–8.69 s).
- GPU waits now poll on a timer (`pollTask`). With the `MessageChannel` yield there, the attach
  spun ~5.7k probe polls in ~0.5 s of worker time; the timer costs ~20 ms on `synced` (All pages
  2,806 → 2,824 ms in two interleaved series) and leaves the core idle during the attach.
- Board (`test:engine-board --strict`, 12 frames, base then new back to back): Vello pan at Fit
  draw p95 14.5/15.7 → 14.3/14.9 ms (Layers open/closed), pan at 100% 8.9/8.5 → 8.5/8.4 ms, zoom
  17.5/16.8 → 18.1/18.8 ms (24 draws, noisy), cold load 828–838 → 827–835 ms, no long task.

Rerun (the base dist must carry the base engine wasm: build it in a worktree of the base commit
with `npm run build:engine-wasm`, then `vite build --outDir <base-dist>` there):

```bash
export REPAINT_SERVER_BIN=target/release/fagent-server
node web/tests/engine-load.mjs --channel chrome --runs 1 --scopes all,pg-1 --dist <base-dist> --label base-1
node web/tests/engine-load.mjs --channel chrome --runs 1 --scopes all,pg-1 --dist <new-dist> --label new-1
node web/tests/engine-load.mjs --channel chrome --runs 3 --scopes all --interact --dist <new-dist> --label interact-new
```

Repeat the first two lines alternately for each round; summaries land in
`artifacts/engine-load/summary-<label>.json` (ignored).

### Measurements — 2026-11-27 (review follow-up, plan cache isolated)

Apple M5 (10 cores, desktop apps running: load average 3.8–6.3), Google Chrome 158 (new headless,
WebGPU), production builds, the 2026-11-26 fixture and method; another machine than 2026-11-26,
so compare ratios, not absolute times. Three builds, five interleaved rounds with the order
rotated: **base** `77e2430`; **no cache**, this branch's viewer with the base engine wasm (byte
identical to base's: the plan cache is the only Rust change); **new**, this branch with the
review fixes. Commit → `drawn` now runs from the worker's `commit` to the page receiving the
first image drawn after it. p50 (min–max), ms from navigation:

| | base All pages | no cache All pages | new All pages | base `pg-1` | no cache `pg-1` | new `pg-1` |
| --- | --- | --- | --- | --- | --- | --- |
| Worker created | 349 (348–365) | 351 (344–352) | 350 (349–363) | 348 (346–355) | 352 (346–354) | 356 (345–377) |
| `/snapshot` request | 349 (347–364) | 351 (344–351) | 350 (348–362) | 347 (346–354) | 351 (345–353) | 355 (344–376) |
| `backend` attached | 840 (821–909) | 836 (820–855) | 861 (827–865) | 734 (727–799) | 745 (728–780) | 736 (722–852) |
| First frame message | 868 (848–941) | 864 (848–887) | 889 (855–893) | 837 (828–904) | 853 (836–897) | 839 (822–959) |
| **`synced`** | **7,222 (7,045–7,550)** | **3,301 (3,274–3,540)** | **2,345 (2,309–2,398)** | **1,440 (1,411–1,521)** | **1,104 (1,089–1,162)** | **1,043 (1,019–1,154)** |
| Last `drawn` | 7,235 (7,057–7,562) | 3,313 (3,290–3,555) | 2,360 (2,323–2,415) | 1,452 (1,422–1,540) | 1,118 (1,103–1,176) | 1,056 (1,032–1,168) |
| Draws while loading | 424 (413–426) | 41 (39–43) | 28 (26–29) | 38 (37–40) | 6 | 7 (6–7) |
| Commit → `drawn` p95 | 14 (14–15) | 107 (106–110) | 102 (101–109) | 13 (12–19) | 96 (93–101) | 108 (102–109) |
| Commit → `drawn` max | 16 (15–18) | 125 (124–137) | 131 (122–137) | 13 (13–20) | 104 (99–113) | 114 (108–115) |
| Worker `render` total | 4,067 | 387 | 286 | 412 | 65 | 88 |
| Worker `compute` total | 2,066 (1,976–2,138) | 1,937 (1,924–2,085) | 1,066 (1,047–1,093) | 236 (230–252) | 230 (225–242) | 137 (134–145) |
| Worker `frame_keys` | 384 | 382 | 388 | 278 | 281 | 278 |
| Worker `required_fonts` | 89 | 69 | 70 | 11 | 9 | 10 |
| Main-thread long tasks | none | none | none | none | none | none |

- Whole patch: All pages `synced` −4.88 s (−68%), `pg-1` −0.40 s (−28%); every new run is faster
  than every base run.
- The plan cache alone (no cache → new, same viewer): `compute` 1,937 → 1,066 ms (−45%) on All
  pages and 230 → 137 ms (−40%) on `pg-1`, `synced` −0.96 s. The viewer changes alone (base → no
  cache) leave `compute` within run spread (−6%, ranges overlap) and cut `render` 4,067 → 387 ms.
- Commit → `drawn` on the new build: p95 101–110 ms, max 122–137 ms (Task 36 AC: p95 ≤ 250 ms).
  The base draws after every frame, hence 14/16 ms at 424 draws.
- The `/snapshot` request starts with the Worker (~350 ms) in every build: Task 36 item 3 is open.
- Zooming while loading (`--interact`, 3 interleaved rounds, then 5 more runs of new): camera →
  `drawn` max 18–34 ms (base 24–26 ms), `synced` 2.42 s (base 7.37 s). The new `cameraOrder`
  check found a frame computed between a camera and its draw in 1 of 3 base runs and in 4 of 4
  runs of the branch before the loader fix, always `load_font:font_lato compute:nav`: the camera
  arrived while the loader awaited a font. The loader now yields after fetching fonts; 0 of 8 runs
  since.
- `test:engine-board --strict` passes on this machine (Vello long tasks none).

Rerun (dists built as in the 2026-11-26 rerun; the no-cache dist from a worktree of this branch
with `crates/text/src` checked out from the base commit and `plans.rs` removed):

```bash
export REPAINT_SERVER_BIN=target/release/fagent-server
node web/tests/engine-load.mjs --channel chrome --runs 1 --scopes all,pg-1 --dist <dist> --label <build>-<round>
node web/tests/engine-load.mjs --channel chrome --runs 1 --scopes all --interact --dist <dist> --label interact-<build>-<round>
```

Builds sharing one cargo target dir across worktrees can reuse a stale engine (cargo compares
source mtimes, and paths are workspace-relative): touch the crate sources after switching trees
and check the engine wasm hash in each dist.

### Measurements — 2026-11-27 (engine scene cache, Task 42)

Apple M5 (10 cores, desktop apps running: load average 1.9–2.7 during the series), Google Chrome
158.0.8127.37 (new headless, WebGPU), production builds, 1440×900 at DPR 2, Layers closed. The
2026-11-26 fixture now paints its white fills with one color token (`paper`, same value, so the
same pixels) for `--edit-shared`. **base** is `66d82f4` (`perf/viewer-large-project`, engine wasm
`16c9ef85d470`), **new** this branch (`e018392ca174`). Cold: five interleaved rounds of one run per
build, each in a new browser context (empty profile, so the new build also creates its database
and writes every record after `synced`). Warm: one persistent profile per series, a cold fill,
then five reloads. Before the cache, a warm series of this fixture equalled cold (All pages warm
`synced` p50 2,217 ms against a 2,233 ms fill; `pg-1` 1,040 against 1,132). p50 (min–max), ms
from navigation:

| | base cold All | new cold All | **new warm All** | base cold `pg-1` | new cold `pg-1` | **new warm `pg-1`** |
| --- | --- | --- | --- | --- | --- | --- |
| Worker created | 347 (346–349) | 346 (343–348) | 337 (333–338) | 349 (346–349) | 351 (346–353) | 344 (340–346) |
| `backend` attached | 813 (803–820) | 817 (808–823) | 436 (431–438) | 736 (731–737) | 721 (713–751) | 435 (431–440) |
| First commit (worker) | 843 (828–847) | 844 (833–850) | 456 (450–457) | 829 (824–831) | 815 (806–846) | 478 (474–481) |
| **`synced`** | **2,203 (2,184–2,294)** | **2,212 (2,189–2,235)** | **560 (555–561)** | **1,026 (1,015–1,031)** | **995 (985–1,035)** | **520 (516–524)** |
| **`synced` − `backend`** | 1,388 (1,381–1,481) | 1,395 (1,380–1,411) | **123 (123–124)** | 292 (284–294) | 270 (268–285) | **84 (83–86)** |
| Last `drawn` | 2,220 (2,200–2,309) | 2,228 (2,205–2,251) | 588 (584–589) | 1,040 (1,028–1,045) | 1,011 (999–1,049) | 535 (531–539) |
| Draws while loading | 25 (24–27) | 25 (24–26) | **2** | 7 | 6 (6–7) | 2 |
| `frame_keys` / `compute` / `import_record` calls | 1 / 229 / 0 | 1 / 229 / 0 | **0 / 0 / 229** | 1 / 23 / 0 | 1 / 23 / 0 | 0 / 0 / 23 |
| Cache hits, MB read, IndexedDB read ms | – | 0, 0, 2 | 229, 14.0, 10 (10–11) | – | 0, 0, 3 (1–10) | 23, 1.4, 7 (1–8) |
| Worker `frame_keys` | 367 | 369 | 0 | 286 | 287 | 0 |
| Worker `compute` total | 1,019 | 1,017 | 0 | 132 | 131 | 0 |
| Worker `import_record` total | 0 | 0 | 51 (49–51) | 0 | 0 | 16 |
| Worker `render` total | 239 | 240 | 76 | 86 | 72 | 50 |
| Font window (requests) | 84 (4) | 86 (4) | 0 (0) | 61 (4) | 62 (4) | 0 (0) |
| Main-thread long tasks | none | none | none | none | none | none |

| | warm `--edit 1` (All) | warm `--edit-shared` (All) | base cold 4× | new cold 4× | **new warm 4×** |
| --- | --- | --- | --- | --- | --- |
| `backend` attached | 799 (792–810) | 793 (778–795) | 458 (439–2,187) | 461 (429–2,185) | 474 (455–489) |
| First commit (worker) | 809 (800–819) | 810 (796–813) | 2,284 (2,223–2,339) | 2,303 (2,225–2,359) | 494 (476–509) |
| **`synced`** | **938 (918–940)** | **2,153 (2,136–2,159)** | **8,564 (8,360–8,684)** | **8,601 (8,448–8,950)** | **870 (861–887)** |
| **`synced` − `backend`** | 136 (126–139) | 1,360 (1,341–1,381) | 8,084 (6,173–8,181) | 8,159 (6,265–8,500) | **396 (395–406)** |
| Last `drawn` | 957 (937–958) | 2,169 (2,151–2,175) | 8,608 (8,404–8,729) | 8,646 (8,492–8,995) | 920 (909–936) |
| Draws while loading | 4 | 25 (24–26) | 79 (74–80) | 79 (78–82) | 5 |
| `frame_keys` / `compute` / `import_record` calls | **1 / 1 / 228** | **1 / 229 / 0** | 1 / 904 / 0 | 1 / 904 / 0 | **0 / 0 / 904** |
| Cache hits, MB read, IndexedDB read ms | 228, 14.0, 11 | 0, 0, 8 | – | 0, 0, 6 (5–6) | 904, 56.2, 39 (39–48) |
| Worker `frame_keys` | 364 | 363 | 1,680 | 1,693 | 0 |
| Worker `compute` total | 15 | 1,018 | 3,956 | 3,965 | 0 |
| Worker `import_record` total | 50 | 0 | 0 | 0 | 162 (160–165) |
| Worker `render` total | 59 | 249 | 1,939 | 1,950 | 254 |
| Font window | 0 | 0 | 147 | 147 | 0 |
| Main-thread long tasks | none | none | 59 (0–63) ms | 60 (0–63) ms | none |

- Warm All pages: `synced` −1,652 ms (−75%) against cold in the same session, `synced` −
  `backend` 123 ms (AC ≤ 300 ms), 2 draws (the attach draw and the last one; AC ≤ 3), every frame
  imported. `backend` attaches ~380 ms earlier because `frame_keys` (367 ms of synchronous worker
  time) no longer runs during the attach; 229 imports take 51 ms (0.22 ms each). Before the font
  cache (item 5) the same series gave `synced` 631 ms and `synced` − `backend` 195 ms with an 86 ms
  font window.
- Cold: within run spread on both scopes (All +9 ms at p50, ranges overlap; `pg-1` −31 ms). The
  new build writes after `synced`: 229 records (14.0 MB) cost 42 ms of export work spread over
  worker yields, 23 records 7 ms.
- Partial (`--edit 1`, the page away while one screen is retitled through MCP): exactly one
  `frame_keys` (the ETag changed), one `compute`, 228 imports and one record written, in every
  run; `synced` 938 ms, `synced` − `backend` 136 ms, 4 draws. `frame_keys` still blocks the attach
  (Task 35 would shrink it).
- Shared edit (`--edit-shared`, a new value of the token every root frame paints with before
  each reload): every frame recomputes and is written again in every run (1 / 229 / 0);
  `synced` 2,153 ms against 2,212 ms cold (−3%, AC within 10%): fonts come from the font cache.
- 4× (`--frames 900 --pages 40`: 904 root frames, 27,057 nodes, 41 pages): warm `synced` 870 ms
  against 8,601 ms cold (−90%), `synced` − `backend` 396 ms (AC ≤ 1 s), 5 draws; 904 imports take
  162 ms and the read 39 ms for 56.2 MB. Cold over six runs per build (three interleaved, three
  sequential): +37 ms at p50 (+0.4%), ranges overlap. The fill wrote 904 records (56.2 MB) after
  `synced` with 167 ms of export work. `backend` lands at ~0.45 s or ~2.2 s in both builds: the
  1.7 s `frame_keys` blocks the attach when it starts first.

Rerun (the base dist as in the 2026-11-26 rerun):

```bash
export REPAINT_SERVER_BIN=target/release/fagent-server
node web/tests/engine-load.mjs --channel chrome --runs 1 --scopes all,pg-1 --dist <dist> --label cold-<build>-<round>
node web/tests/engine-load.mjs --channel chrome --scopes all,pg-1 --runs 5 --warm --label warm
node web/tests/engine-load.mjs --channel chrome --scopes all --runs 5 --warm --frames 900 --pages 40 --label warm-4x
node web/tests/engine-load.mjs --channel chrome --scopes all --runs 5 --warm --edit 1 --label warm-edit
node web/tests/engine-load.mjs --channel chrome --scopes all --runs 5 --warm --edit-shared --label warm-shared
node web/tests/engine-load.mjs --channel chrome --runs 1 --scopes all --frames 900 --pages 40 --dist <dist> --label cold4x-<build>-<round>
```

Summaries land in `artifacts/engine-load/summary-<label>.json` (ignored); each warm summary keeps
its fill run (`results.<scope>.fill`) and per-run traces of the load window.

### Tests

- `npm --prefix web test` includes `engine-runtime.test.ts` (renderer choice, the WebGPU → WebGL2 →
  vello_cpu → Canvas2D chain, probe rejection, device loss rebuild-once; paced draws with a fake
  clock: first frame at once, later ones per gap, the last
  always before `synced`, held `frame` messages posted before their `drawn`; a camera or layout
  during a load drawn unpaced without releasing held frames; a failed last draw holds its frames
  and `synced` until the next backend draws them, and a reused frame waiting for that image
  waits with it; `progressGap` bounds; GPU waits poll on
  `pollTask`, never on the frame yield), `engine-client.test.ts` (page side: a fresh canvas per
  fallback step, the snapshot sent once per revision and again with a new ETag, image placement,
  hidden layouts, overscan) and `engine-frames.test.ts` (frame loader: order, reuse,
  missing font/asset, supersede, a yield after a font fetch before compute), and
  `engine-cache.test.ts` (scene cache with an in-memory store: a reload imports every frame
  without `frame_keys` or `compute`, an edited frame recomputes alone, another build misses, a
  damaged record or font list computes and is replaced, read timeout and write quota fall back
  to compute, Retry bypasses, a newer sync aborts the older read, nothing is read before a sync
  carries an answered snapshot, a held sync with a new ETag neither reads nor files digests
  under it, a sync superseded during its manifest read runs no `frame_keys`, a damaged manifest
  is a miss, a manifest hit is written again, an oversized record is counted apart, a font
  failure of a superseded sync or of a font only a record lists is not remembered; LRU
  eviction, restored-frame pacing (the first computed frame after restored ones goes at once),
  the font cache with its byte bound, a retried open and a put over quota).
- `npm --prefix web run test:engine-cache` (`engine-cache-browser.mjs`, real Chromium IndexedDB;
  `FAGENT_TEST_CHANNEL=chrome` for Chrome): one read transaction, misses and damaged records or
  manifests, LRU with touched records (also touched and added in one batch at capacity, with no
  usage row left without its record), manifests, removal, an aborted read, another build's
  first write removing the old rows, and once per worker only.
- `cargo test -p fagent-text` covers the shape plan cache (`crates/text/src/plans.rs`): reuse only
  for the same script and features, clones share, fonts never share, cached plans shape exactly
  like fresh ones.
- `npm --prefix web run test:engine-load` (`web/tests/engine-load.mjs`) measures the cold load of
  a large synthetic project (see the 2026-11-26 and 2026-11-27 measurements for options). Commit →
  `drawn` runs from the worker's `commit` call to the page's receipt of the image of the first
  draw after it; with `--interact` it checks that no frame computation starts between a camera's
  `set_camera` and its draw (`cameraOrder`). `--frames`, `--pages`, `--runs` and `--latency` must
  be integers (`--pages` ≤ `--frames`). `--warm` (Task 42) uses one persistent profile per scope
  in a temporary directory: a cold navigation fills the cache, the runner waits for its writes,
  then `--runs` reloads are measured; `--edit <k>` retitles k screens through MCP before each
  reload (the page is away meanwhile, so no live update computes them first), `--edit-shared`
  changes the token every root frame paints with. Each run also reports `synced − backend`, engine
  call counts (`frame_keys`, `compute`, `import_record`, …), the worker's cache counters and the
  font window (first font request to the last `load_font`). Routing the glue (for the engine
  timers) turns the browser HTTP cache off, cold and warm alike.
- `cargo test -p fagent-engine-wasm`: compute summary equals the JSON scene path, staging,
  hit test (clip ancestors, revision, board offsets), reuse, layout release, drawing with backdrop,
  culling and LOD through `vello_cpu`; frame records: an imported frame equals the computed one
  in summary (but revision), hit tests, bounds and `vello_cpu` pixels of a board draw (byte
  identical), the record's scene equals the JSON path's, records of another format, project,
  frame, viewport or digest are refused, a missing font is an error, floats read back bit for bit
  (200,000 sampled `f32` patterns).
- `npm --prefix web run test:engine` (real Chromium): WebGPU paint, hit test and Inspector equal to
  Canvas2D, screenshot diff, overscan (the test fulfils `/api/v1/viewer` with `overscan: 0.1`; a pan
  within the margin while camera messages are held shows frame pixels in the exposed strip, equal
  to the redrawn image), long tasks, device loss → rebuild → WebGL2, forced WebGL2/vello_cpu, no
  GPU → vello_cpu, module failure → Canvas2D. `npm --prefix web run test:engine-board` measures the
  board (fails on a > 50 ms canvas-path long task). `FAGENT_TEST_RENDERER=vello node
  web/tests/node-actions-smoke.mjs` runs the export/copy/context-menu smoke on the engine.
  Without the pinned browsers in `.runtime/browsers`, `FAGENT_TEST_CHANNEL=chrome` runs these
  (GPU and GPU-less cases) on an installed Google Chrome.

**Open (outside P1):** tile cache so Fit composites instead of re-encoding (Task 31); Safari 26 and
slower machines before any default change; image mipmaps at very small zoom; glyph-atlas eviction
under stress. Missing font bytes make a frame unavailable in this mode (the Canvas2D path can fall
back to a server scene with placeholders).

## Verification — 2026-10-28

Commands actually run from the workspace:

```sh
npm --prefix web run typecheck
npm --prefix web test
npm --prefix web run build
node web/tests/smoke.mjs
```

- TypeScript check passed. Production Vite build passed; its approximately 574 kB JavaScript bundle currently emits the normal 500 kB chunk-size warning.
- Four state tests passed: numeric layer sorting, valid/deleted selection reconciliation, SSE duplicate/gap decisions, and rejection of old/mislabeled jobs and viewport/source-key races.
- Playwright Chromium 156.0.8078.4 smoke passed: GET-only requests; selection persistence over reload; zoom and keyboard fit; late job R after R+1; SSE gap/reset/reconnect; missing font and missing asset; failed worker state; deleted layer fallback; and no horizontal overflow at 390 px. The original pending assertion hid the old canvas; the continuity regression now checks a retained canvas with an explicit old-revision label instead. No browser page errors occurred.
- Actual UI screenshots and machine-readable evidence are under `artifacts/viewer/`: `desktop.png`, `narrow.png`, `missing-resources.png`, `evidence.json`. Screenshots use an explicitly named in-memory **Viewer test fixture**, never a real user project. Its temporary API server uses a random localhost port and shuts down after the test. Port 3847 remains free for the backend.

The smoke test verifies the frontend HTTP contract against a temporary fixture; it does not prove live Rust/MCP integration, persistence, or PNG equality. Those require the backend integration acceptance run. No claim is made here that M1/M2 are complete end to end.

All hand-written viewer source and test files were split by responsibility and are below 250 lines after normal formatting. `package-lock.json` is generated. No separate design-system project was created.

## Live backend acceptance — resumed run

The later production E2E closes the live-integration gap described above. The
worker ran `npm run test:e2e`; root independently rebuilt the final backend and
ran `node spikes/e2e/run.mjs` successfully on 2026-10-28. Actual MCP calls created
the temporary SQLite project and resources; the production viewer received the
same computed scene/line list/provenance as strict PNG. Decoded RGBA differences
were zero at 640×480 for revisions 2/3 and after process restart, and at 390×844.
The browser updated through production SSE after a token mutation without manual
refresh. Restart/reload preserved IDs, contents, resource references and selection.
Selection, keyboard pan, zoom, Fit, narrow layout and GET-only viewer API traffic
were checked. Root inspected actual desktop/mobile screenshots. All temporary
data and harness processes were cleaned up; no project used `~/.fagent`.

See `artifacts/e2e/evidence.json`, `spikes/e2e/README.md` and `docs/ACCEPTANCE.md`
for exact evidence and limits. The earlier fixture checks remain evidence for
controlled stale-job/reconnect/failure UI scenarios; the live run does not claim
cross-browser pixel equality or replace backend/storage failure suites.

## Motion preview

`/preview?project=<projectId>&frame=<frameId>[&width=&height=]` plays a frame's declared
`props.animations`, `props.states` and `props.links` in the browser. The header's **Preview**
button opens it for the selected frame in a new tab; the server's SPA fallback serves it, and
`web/src/main.tsx` picks `PreviewApp` for paths under `/preview`. It is a DOM preview, not a
time-sampled render (P07c/P07d stay out of scope), and it sends only GET requests.

**Data.** The authored snapshot gives animations, links, states, tokens/styles and the root
frame picker. The default-state scene (`state=default`, default theme) gives bounds, fill,
opacity, radius, stroke, text lines, fonts and assets. For each state declared in the frame
(including masters of its instances) the scene is also fetched with `state=<name>`; per-node
differences against the default scene become the state visuals, so the server keeps owning
theme/token resolution. Scene jobs share `fetchSceneJob` (`web/src/sceneJob.ts`) with
`useScene`. Width/height default to the authored frame size (800×600 fallback).

**DOM.** One absolutely positioned element per scene node (`.oc-node` → `.oc-box` for fill,
radius, clip and children, then `.oc-stroke`, a border overlay outside the node's own clip,
sized like `renderer/stroke.ts`). Text is SVG `<text>` at the scene's baseline positions, so the
browser never re-wraps; images use the shared `placeImage` with crop. The 1:1 frame is scaled
down to fit the window. At rest it differs from the worker PNG in under 1% of pixels (text
antialiasing) on the smoke fixture.

**Motion semantics** (`web/src/preview/keyframes.ts`, unit tested):

- `fillMode` defaults to `both`; times are milliseconds; `"infinite"` iterations map to
  `Infinity`. The effect is linear and each keyframe carries `easing ?? animation.easing ?? ease`
  for the segment starting at it, as in CSS.
- Offsets spread like WAAPI (lone keyframe without offset = 1). Every animated property missing
  at offset 0 or 1 gets an explicit keyframe with the node's resting scene value.
- `translateX/Y`, `scale`, `rotate` map to the CSS `translate`, `scale`, `rotate` properties
  (around the bounds center), one CSS property per validator channel, so two animations on one
  node animating different channels never clobber a shared `transform`.
- `fill` animates `background-color` of shapes and `color` of text (SVG text uses
  `currentColor`). `{tokenId}`/`{styleId}` resolve at the default theme from the snapshot; an
  unresolvable reference skips that property and is listed under the stage as a warning.
- `load` plays when the frame mounts; **Replay** resets every animation and restarts the load
  ones. `hover` plays forward on `pointerenter` and reverses from its current time on
  `pointerleave`. By default the animated node receives these events. Set
  `triggerNodeId` to an ancestor group or frame in the same root frame to make its hover
  area drive one or more descendants' animations. Moving between descendants or across gaps
  inside the source keeps those animations active. Explicit triggers ignore touch pointer
  events. A missing source produces a warning and skips its animation. `click` restarts on
  every click.
- **Reduced motion** starts from `prefers-reduced-motion` and can be toggled: triggers jump to
  their end state (`finish()`), an infinite animation stays at rest, hover leave returns to rest.

**States.** Rules are generated per declared state with `!important` on `:hover`, `:active`
(pressed), `:focus-visible` (focus; such nodes get `tabindex=0`) and `[data-oc-disabled]`
(the **Disabled state** toggle, shown only when declared). They therefore beat the resting
inline styles. Animations own `translate`/`scale`/`rotate`; for `opacity` and `fill`, an active
state that declares the property wins over the animated value, and the animation shows again
when the state ends. A state stroke replaces the whole stroke overlay.

**Links.** A click link `navigate` pushes `?frame=<to>` and shows that frame; `back` goes back
through preview history (a toast when there is none). `overlay`/`close` show a "not supported in
preview" toast; hover (overlay) links are listed as warnings.

**Instances (P24).** Scene IDs of instance children are virtual (`<instanceId>/<masterChildId>`).
State visuals come from the per-state scenes, so expanded master states work without ID
mapping. Component subtrees and instances cannot author animations or links (storage rejects
them), so there is nothing else to map.

**Known limitations.** Default design theme only; no overlay/close navigation; states are
plain CSS pseudo-classes (hovering a child also hovers its ancestors, focus needs keyboard
focus); the preview loads one snapshot and does not follow live SSE commits (use Reload); a
`width`/`height` in the URL is kept across link navigation.

Tests: `npm --prefix web test` (keyframes, model and state-style units) and
`npm --prefix web run test:preview`, a Playwright smoke against a temporary server
(`target/debug` binaries, `renderer/dist`, `web/dist` must be built) with a temporary data root,
which creates its fixture through MCP and removes everything afterwards.
