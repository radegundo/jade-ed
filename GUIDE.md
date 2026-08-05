# jade-ed — Architecture Guide

A comprehensive walkthrough of the `jade-ed` codebase: every module, the data
model, how the two modes work, how picking/dragging is wired, how the tools
mutate the map, and how to extend the editor. Line references point at the
current source; where a comment in the code contradicts this document, trust
the code (the guide was written against `git@4d51d32`).

## Table of contents

1. [Overview](#overview)
2. [Running the project](#running-the-project)
3. [Controls](#controls)
4. [Module map](#module-map)
5. [The map data model (`map.rs`)](#the-map-data-model-maprs)
6. [Modes and cameras (`mode.rs`)](#modes-and-cameras-moders)
7. [The 3D fly camera (`viewport.rs`)](#the-3d-fly-camera-viewportrs)
8. [Lighting (`scene.rs`)](#lighting-sceners)
9. [Mouse picking (`picking/`)](#mouse-picking-picking)
10. [Vertex handles (`map_handles.rs`)](#vertex-handles-map_handlesrs)
11. [2D overlay gizmos (`map_gizmos.rs`)](#2d-overlay-gizmos-map_gizmosrs)
12. [Editing tools (`tools.rs`)](#editing-tools-toolsrs)
13. [3D height editing (`height_handles.rs`)](#3d-height-editing-height_handlesrs)
14. [The 3D preview (`map_preview.rs`)](#the-3d-preview-map_previewrs)
15. [The egui toolbar (`ui.rs`)](#the-egui-toolbar-uirs)
16. [Application wiring (`main.rs` / `editor.rs`)](#application-wiring-mainrs--editorrs)
17. [Design decisions](#design-decisions)
18. [The portal algorithm](#the-portal-algorithm)
19. [Test coverage](#test-coverage)
20. [Recent change log](#recent-change-log)
21. [Extending the editor](#extending-the-editor)

## Overview

`jade-ed` is a Bevy 0.19 map editor prototype for a Doom-style sector map
model. The map is a set of **2D floor polygons** (`Sector`s, on the XZ plane)
with floor/ceiling heights, plus prism-shaped **obstacles** and **portals**
(walls shared between two sectors). You edit the map in a top-down 2D view and
walk through it in a textured 3D view. Press **Tab** to switch.

```
src/
  main.rs             App + window + module registration
  editor.rs           EditorPlugin: composition root listing every sub-plugin
  map.rs              Map data model, builders, runtime editing, tests
  mode.rs             Mode state, camera spawning, 2D camera controls
  viewport.rs         3D fly camera + InfiniteGrid
  scene.rs            DirectionalLight
  picking/            Mouse picking + drag system
    mod.rs            OwnPickingPlugin + hover observers
    state.rs          PickingState: per-frame picking snapshot
    controller.rs     update_picking_state (fills PickingState)
    drag.rs           Click-threshold drag pipeline
    visuals.rs        Hover/selected/drag tints
  map_handles.rs      2D pickable vertex-handle spheres
  map_gizmos.rs       Wall/portal/vertex lines drawn in 2D
  tools.rs            Draw sector/wall/obstacle tools, selection, delete
  height_handles.rs   3D height editing (sector/obstacle handles, body drag)
  map_preview.rs      3D preview: map data -> textured meshes/materials
  ui.rs               egui "Editor" toolbar window
```

Dependencies (`Cargo.toml`): `bevy` 0.19 (features `dynamic_linking`,
`debug`, `bevy_dev_tools`), `bevy_egui` 0.41.1, `bevy-inspector-egui` 0.37.

## Running the project

```sh
cargo run
```

Run with `cargo run` from the project root. Launching the built binary directly
(`./target/debug/jade-ed`) fails because `dynamic_linking` requires the
`libbevy_dylib` shared library to be found.

Run the test suite:

```sh
cargo test
```

The suite currently has **22 tests** (18 in `map::tests`, 4 in
`height_handles::tests`). All tests are pure unit tests — no window is opened.

## Controls

| Mode   | Action                     | Input                                       |
| ------ | -------------------------- | ------------------------------------------- |
| any    | Toggle 3D / 2D             | `Tab`                                       |
| 3D     | Move                       | `W A S D` (ground-plane, FPS-style)         |
| 3D     | Fly up / down              | `E` / `Q`                                   |
| 3D     | Look around                | Hold right mouse button (locks cursor)      |
| 3D     | Boost speed                | `Shift` or `Space`                          |
| 3D     | Move forward impulse       | Scroll wheel                                |
| 3D     | Select sector / obstacle   | Left click (Select tool)                    |
| 3D     | Drag height handle         | Left drag green (lower) / blue (upper) handle |
| 3D     | Drag obstacle body up/down | Left drag the obstacle's side               |
| 2D     | Pan                        | Middle, or right, or `Space` + left         |
| 2D     | Zoom                       | Scroll wheel (smoothly eased)               |
| 2D     | Auto-pan at window edge    | Cursor near any window edge                 |
| 2D     | Drag a vertex              | Left mouse on a handle (1-unit snap; hold `Alt` to skip) |
| 2D     | Select                     | Left click (Select tool)                    |
| 2D     | Delete selection           | `Delete` or `Backspace`                     |
| 2D     | Nudge height              | `[` / `]` (floor or obstacle bottom), `Shift+[` / `Shift+]` (ceiling or top) |
| 2D     | Draw sector               | Toolbar **Draw Sector** + click vertices, right-click/Enter to close, Esc to cancel |
| 2D     | Draw wall                 | Toolbar **Draw Wall** + click start then end, right-click/Esc to cancel |
| 2D     | Place obstacle            | Toolbar **Obstacle** + drag a rectangle inside a sector |

Draw/stamp tools only act in 2D mode; in 3D they just show a yellow reminder in
the toolbar.

## Module map

The composition root is `EditorPlugin` in `editor.rs:16`, which adds every
feature plugin in one tuple:

```
ViewportPlugin   ScenePlugin   UiPlugin   OwnPickingPlugin
ModePlugin       MapPlugin     MapHandlesPlugin   MapGizmosPlugin
MapPreviewPlugin HeightHandlesPlugin   ToolsPlugin
```

System ordering concerns: `MapPlugin` seeds the map at `Startup`
(`setup_map_assets` then `setup_map`, chained). Everything else reacts to
`Map` changes through Bevy change detection. `OwnPickingPlugin` populates
`PickingState` in `PreUpdate` so every `Update` system can read it.

## The map data model (`map.rs`)

`src/map.rs` (1692 lines) is the heart of the editor. It holds the map data
structures, the builders, the runtime editing API, and all unit tests.

### Coordinate conventions

- The map lives on the **XZ plane**; a `Vec2` is `(x, z)`. The 2D camera looks
  straight down `-Y`; the 3D fly camera walks on `y = 0` (the floor of most
  sectors).
- Sectors are wound **counter-clockwise** (`ensure_ccw`, `map.rs:586`), so the
  sector interior is always to the **left** of the wall traversal. Portal walls
  in the two adjacent sectors traverse the shared edge in opposite directions.
- World units == map units; a sector at `(0,0)-(100,100)` is 100 units square.
  The draw tools snap to a 1-unit grid and to existing vertices within a
  1-unit radius.

### Indexed vertex pool

The map is an **indexed vertex pool**: every `LineDef` stores `start_idx` /
`end_idx` into `Map.vertices` rather than raw positions. `add_vertex`
(`map.rs:731`) de-duplicates identical positions, so two walls sharing a corner
or a portal edge automatically share pooled vertices. `rebuild_vertices`
(`map.rs:388`) re-dedups the entire pool from all walls/obstacle edges (used by
deletions).

### Core types

- `Map { vertices: Vec<Vec2>, sectors: Vec<Sector> }` — the `Resource`.
- `Sector` (`map.rs:652`) — `walls`, `obstacles`, `floor_height`,
  `ceiling_height`, `floor_texture`, `ceiling_texture`, `id`. The setters
  `set_floor_height` / `set_ceiling_height` clamp so `floor <= ceiling`.
- `Obstacle` (`map.rs:675`) — `id`, `edges` (a mini wall list), `bottom`, `top`,
  `side_texture`, `top_texture`, `bottom_texture`. Setters clamp `bottom <= top`.
- `LineDef` (`map.rs:710`) — `start_idx`, `end_idx`, `front_side_def`,
  `back_side_def`, `id: WallId`. A `back_side_def` present is what makes a line
  a **portal**.
- `SideDef` (`map.rs:740`) — `textures: SideDefTextures`, `facing` (the sector
  id this side belongs to). `SideDefTextures` (`map.rs:752`) has the classic
  `upper` / `middle` / `lower` texture slots.
- `WallId` (`map.rs:697`) — `{ sector, index }`, used as a stable handle for
  a wall within a sector.

### Static builders (test-map / asset construction)

- `wall(...)` (`map.rs:761`) — single one-sided wall with the middle texture set.
- `portal(...)` (`map.rs:784`) — two-sided wall: upper+lower textures, `front`
  and `back` sector ids.
- `SectorBuilder` (`map.rs:1010`) — chains `.wall()`, `.portal()`,
  `.obstacle()`, `.rect_obstacle()`, then `.build() -> Sector`.
- `ObstacleBuilder` (`map.rs:922`) + `rect_obstacle` (`map.rs:986`) — build a
  box. Its edges are wound **clockwise** so `interior_normal` produces
  **outward**-facing normals for the side quads.
- `rect_sector` (`map.rs:1137`) — convenience 4-wall sector (kept `#[allow(dead_code)]`).

### Runtime builders (used by the 2D tools)

The 2D tools do **not** use the static builders; they call methods on `Map`
that validate input, auto-detect portals (including **partial** edge sharing),
and can mutate existing sectors:

- `add_sector_from_polygon(points, assets) -> Result<usize, String>`
  (`map.rs:77`) — the main draw-tool entry point. Validates the polygon,
  normalizes winding, plans every edge's overlaps up front, splits any
  overlapped walls, builds the new sector (splitting each edge at overlap
  boundaries), and turns matching pieces into portals. Returns the new sector's
  id. Rejections (returned as `Err`) leave the map completely untouched.
- `add_wall(sector_id, start, end, assets) -> Result<(), String>` (`map.rs:190`)
  — adds a single wall to an existing sector with the same overlap/portal
  logic. Rejects walls in the same sector ("Wall already exists in this
  sector") and degenerate walls.
- `add_obstacle(sector_id, x0, y0, x1, y1, assets) -> Result<usize, String>`
  (`map.rs:285`) — places a rectangular obstacle with default heights.
- `remove_sector(id)` (`map.rs:328`) — removes a sector and scrubs the
  back-side from any neighbor portals (they become solid again).
- `remove_obstacle(sector_id, obstacle_id)` (`map.rs:349`).
- `remove_vertex(vertex_idx)` (`map.rs:368`) — removes a pooled vertex and every
  wall/obstacle edge using it; sectors left with <3 walls (and obstacles with
  <3 edges) are dropped too.

### Queries

- `find_sector_at(pos)` (`map.rs:412`) — innermost sector whose polygon
  contains the point (last match in draw order wins), for obstacle placement
  and click selection.
- `find_sector_containing_or_on_edge(pos, tolerance)` (`map.rs:426`) — like
  `find_sector_at`, but also matches sectors whose *boundary vertex* is within
  tolerance (so clicking on a shared corner still resolves a sector). Portal
  edges are ambiguous, so the sector whose centroid is closest wins.
- `find_wall_at_edge(a, b)` (`map.rs:452`) — exact vertex match on either
  orientation; used to detect portal pieces after splitting.
- `sector_centroid(idx)` (`map.rs:559`) — average of wall start points; used by
  the 3D height handles and gizmos.
- Free functions: `point_in_polygon` (ray-cast, `map.rs:1161`),
  `point_in_sector` (`map.rs:1180`), `obstacle_center` (`map.rs:1186`),
  `find_player_sector` (`map.rs:1200`, kept for a future raycaster).

### Assets and the seed map

- `MapAssets` (`map.rs:32`) is a `Resource` of shared texture handles: `wall`,
  `floor`, `ceiling`, `obstacle_side/top/bottom`. `setup_map_assets`
  (`map.rs:41`) loads `texture.png` and `floor_texture.png` with a **Repeat**
  sampler (see [Texture tiling](#texture-tiling)) and aliases them into all six
  slots.
- `test_map` (`map.rs:1211`) builds the demo map with `SectorBuilder`:
  - Sector 0: the 100×100 unit room from `(0,0)` to `(100,100)`, floor 0,
    ceiling 20, with a box obstacle `(40,40)-(50,50)` (bottom 0, top 8) and a
    floating platform `(60,70)-(80,90)` (bottom 5, top 7).
  - A portal on the east wall from `y=40` to `y=60`.
  - Sector 1: a 40×20 side room `(100,40)-(140,60)`, floor **10**, ceiling 20,
    with the matching portal on its west wall.
- `setup_map` (`map.rs:1267`) inserts it as the `Map` resource at startup.

### Constants

`map.rs:14` — `DEFAULT_FLOOR_HEIGHT = 0.0`, `DEFAULT_CEILING_HEIGHT = 20.0`,
`DEFAULT_OBSTACLE_BOTTOM = 0.0`, `DEFAULT_OBSTACLE_TOP = 8.0`. Everything
created through the 2D tools uses these; heights are edited afterwards with the
3D handles, `[`/`]` nudging, or the egui drag values.

## Modes and cameras (`mode.rs`)

`mode.rs` (352 lines) owns the single source of truth for which mode is active,
spawns both cameras, handles the Tab switch, and implements the 2D camera
controls.

### State and tags

- `ModeState { mode: EditorMode }` (`mode.rs:14`) + `EditorMode::{View3D, Edit2D}`
  (`mode.rs:20`, default `View3D`).
- `Camera3D` / `Camera2D` tag the two cameras (`mode.rs:27` / `:31`).
- `VisibleIn3D` / `VisibleIn2D` tag entities that should appear in only one mode
  (`mode.rs:36` / `:39`).
- `Camera2DZoom { target_scale }` (`mode.rs:43`) — the eased zoom target.
- `Camera2DEdgePan { velocity }` (`mode.rs:49`) — edge-pan velocity.
- `in_mode(mode)` (`mode.rs:67`) — run condition closure used everywhere:
  `in_mode(EditorMode::Edit2D)` gates 2D systems, `in_mode(EditorMode::View3D)`
  gates 3D systems.

### Camera spawning

`spawn_cameras` (`mode.rs:73`, `Startup`):

- The **3D camera** starts active with a `FlyCamera`-derived transform, an
  `EguiContext`, and `PrimaryEguiContext` (the egui host at startup).
- The **2D camera** starts hidden with an orthographic projection
  (`ScalingMode::WindowSize`, `scale` 0.15), a `Camera2DZoom`, a
  `Camera2DEdgePan`, an `EguiContext`, and `EguiContextSettings
  { capture_pointer_input: false }`. It looks straight down with **up = `-Z`**
  (un-mirrored: screen-right = world `+X`, screen-up = world `-Z`), so it has
  the same handedness as the map data.

Both cameras keep an `EguiContext` **permanently** — the rationale is in
[Two cameras, one egui context](#two-cameras-one-egui-context).

### Toggling (`toggle_mode`, `mode.rs:123`)

On Tab:

1. Restores the cursor (`CursorGrabMode::None`, visible) so a locked 3D
   look-drag can't strand it hidden.
2. Flips `ModeState`.
3. Sets `Camera.is_active` + `Visibility` on both cameras.
4. Sets `EguiContextSettings.capture_pointer_input` on both cameras (true on
   the winner, false on the loser).
5. Moves `PrimaryEguiContext` + `EguiMultipassSchedule` to the active camera
   (removing them from the inactive one). `EguiContext` itself is never
   added/removed (see design decisions).
6. Toggles `Visibility` for all `VisibleIn2D` / `VisibleIn3D` entities.
7. **Clears drag state** (`DragState` and `ObstacleDrag`) and strips
   `BeingDragged` from any entity, so a mid-drag Tab can't cause cross-mode
   ghost dragging.

### 2D camera controls (`control_2d_camera`, `mode.rs:227`)

- **Zoom**: scroll adjusts `Camera2DZoom.target_scale` (line vs pixel units),
  clamped to `0.02..5.0`; the actual `ortho.scale` eases toward it
  (`ease = 1 - exp(-12*dt)`).
- **Pan**: middle mouse, or right mouse (when the current tool doesn't claim
  right-click — see the tool gating at `mode.rs:260`), or `Space` + left. The
  camera moves **opposite** the cursor (`translation.x -= motion.x * scale`,
  `translation.z -= motion.y * scale`) so the content follows the cursor with a
  1:1 grab feel.
- **Edge pan**: cursor within 60 px of a window edge auto-pans at
  `400 * scale` units/sec.

### Framing (`center_2d_camera_on_map`, `mode.rs:312`)

Runs once (via a `Local<bool>`) when the map is first available: frames the
map's bounding box with 1.2× padding, choosing the ortho scale so the map fills
the viewport height.

## The 3D fly camera (`viewport.rs`)

`ViewportPlugin` (`viewport.rs:12`) adds the `InfiniteGridPlugin` plus
`CameraPlugin` and `GridPlugin`.

`FlyCamera` (`viewport.rs:33`) stores `position`, `yaw`, `pitch`, `speed`,
`fast_speed`, `sensitivity`. The `Transform` is derived each frame from the
state (`viewport.rs:135`):

```rust
transform.translation = fly.position;
transform.rotation = Quat::from_axis_angle(Vec3::Y, fly.yaw)
                   * Quat::from_axis_angle(Vec3::X, fly.pitch);
```

`control_fly_camera` (`viewport.rs:58`, gated to `View3D`):

- **FPS-style movement**: forward is horizontal-only
  (`Vec3::new(-sin(yaw), 0, -cos(yaw))`), so `W` never moves into the sky
  regardless of pitch.
- `Q`/`E` move straight up/down.
- Right mouse locks and hides the cursor while looking (`CursorGrabMode::Locked`);
  pitch is clamped to ±(π/2 − 0.01).
- `Shift` or `Space` boost speed (20 → 60); scroll wheel gives a one-shot
  forward nudge.

`GridPlugin` (`viewport.rs:140`) spawns Bevy's `InfiniteGrid` at `y=0` with a
150-unit fadeout and red X / green Z axis colors.

## Lighting (`scene.rs`)

`ScenePlugin` spawns a single `DirectionalLight` (illuminance 5000) shining
down the `(-1,-2,-1)` direction (`scene.rs:11`). This plus Bevy's ambient light
is the entire lighting setup — the 3D preview relies on it.

## Mouse picking (`picking/`)

The picking module is the pipeline everything else reads. It uses Bevy's
built-in `MeshPickingPlugin` plus hand-rolled camera-ray math.

### `state.rs` — `PickingState`

`PickingState` (`state.rs:6`) is the single per-frame snapshot:

- `camera_ray: Option<Ray3d>` — ray from the active camera through the cursor.
- `ground_hit` / `ground_hit_snapped` — intersection with the `y=0` plane,
  raw and snapped to the 1-unit grid (`snap_to_grid`, `state.rs:38`, snaps X/Z,
  preserves Y).
- `hovered_entity`, `mesh_hit_point`, `mesh_hit_normal` — nearest mesh hit from
  `MeshPickingPlugin`.
- `just_pressed` / `just_released` / `is_pressed` — left button states.
- `cursor_pos` / `cursor_pos_prev`, and `shift_held` / `ctrl_held` / `alt_held`.

### `controller.rs` — `update_picking_state`

Runs in `PreUpdate` (`controller.rs:6`). Picks the active mode's camera, computes
cursor/button/modifier state, builds the camera ray via `viewport_to_world`, and
finds the `y=0` ground intersection. The mesh-hover info is copied out of the
`PointerInteraction` query results.

### `drag.rs` — the drag pipeline

- `BeingDragged { grab_offset, start_position }` (`drag.rs:6`) — marker on the
  entity while dragging.
- `DragState { entity, press_screen_pos, is_dragging }` (`drag.rs:13`) —
  global drag tracking resource.
- `on_press` (observer, `drag.rs:22`) — fires on `Pointer<Press>`: ignores
  non-primary buttons, non-Select tools, `Space`+left (reserved for pan), and
  non-mesh entities; records the entity + press position but does **not** start
  dragging yet.
- `update_drag` (`drag.rs:61`, gated to `Edit2D`) — each frame:
  - Aborts if the tool is no longer Select or `Space` is held.
  - On release, snaps the entity to the grid (unless `Alt`), removes
    `BeingDragged`, and resets state.
  - Once the cursor passes the 5 px `DRAG_THRESHOLD`, computes the grab offset
    (transform − mesh hit point) and inserts `BeingDragged`.
  - Moves the entity each frame to the snapped ground hit + grab offset,
    preserving `start_position.y` (so 2D handles stay on the ground plane).

### `visuals.rs` — hover/select/drag tints

Two chained systems (`visuals.rs:22`, `:46`) tint marked entities without
losing the original material:

- `restore_unmarked_materials` puts the cached original material back on any
  entity that lost all three markers.
- `apply_material_tints` blends the original color toward a tint — yellow when
  `BeingDragged`, orange when `Selected`, blue when `Hovered` (light orange when
  selected+hovered). The original material handle is remembered in the
  `MaterialCache` resource.

### `mod.rs` — `OwnPickingPlugin`

Adds `MeshPickingPlugin`, the resources, the hover observers
(`on_hover_enter`/`on_hover_exit`), `on_press`, `update_picking_state`
(PreUpdate), the tint chain (Update), and `update_drag` gated to `Edit2D`.

## Vertex handles (`map_handles.rs`)

`map_handles.rs` (109 lines) keeps one pickable green sphere per pooled vertex
in sync with the map:

- `VertexHandle { index }` (`map_handles.rs:8`) — component mapping an entity to
  a pooled-vertex index.
- `sync_handles` (`map_handles.rs:29`, gated to `Edit2D`) — reconciles handle
  entities with `Map.vertices`: moves existing ones to their target, spawns
  missing ones, despawns removed ones. Entities `BeingDragged` are skipped (their
  transform is mid-drag) but still counted so they aren't duplicated. The sphere
  mesh/material are created once and cached in `Local`s.
- `sync_dragged_to_map` (`map_handles.rs:99`) — writes a dragged handle's X/Z
  back into `Map.vertices` (Y stays 0), so moving a handle in 2D edits the map
  that the 3D preview renders.

## 2D overlay gizmos (`map_gizmos.rs`)

`draw_map_gizmos` (`map_gizmos.rs:15`, gated to `Edit2D`) draws pure gizmos each
frame from `Map` data — no entities:

- Walls: gray for solid lines, **blue** for portals (`back_side_def.is_some()`),
  drawn 0.01 above the ground to avoid z-fighting with the grid.
- Green vertex crosses at 0.02 as backup markers under the handle spheres.

## Editing tools (`tools.rs`)

`tools.rs` (599 lines) implements everything the toolbar offers: the draw
tools, obstacle stamping, selection, deletion, height nudging, and the tool
gizmos.

### Tool state and resources

- `ToolState { tool, message }` (`tools.rs:18`) — the active `EditorTool`
  (`Select`, `DrawSector`, `DrawWall`, `PlaceObstacle`) plus a status/error
  message shown in the toolbar.
- `tool_is(tool)` (`tools.rs:35`) — run condition for a specific tool.
- Draft resources: `SectorDraft { points }` (`tools.rs:42`), `WallDraft {
  start }` (`tools.rs:48`), `ObstacleStamp { start, current }` (`tools.rs:55`).
- `Selection { entity, sector, obstacle }` (`tools.rs:63`) — exactly one of the
  three is set: an entity (vertex handle / obstacle marker), a sector *index*,
  or an obstacle `(sector_id, obstacle_id)`.
- `ObstacleHandle { sector_id, obstacle_id }` (`tools.rs:71`) — pickable marker
  entity at an obstacle's centroid for select/drag/delete.

### Snap constants

`SNAP_RADIUS = 1.0` (`tools.rs:13`) — how close a click must be to an existing
vertex to snap to it (this is what makes shared corners exact so portals form).
`CLICK_DRAG_THRESHOLD = 5.0` (`tools.rs:14`) — a click is a click only if the
cursor moved less than this; otherwise it's a drag.

### `draw_sector_tool` (`tools.rs:113`)

- Left click appends a snapped point to `SectorDraft.points`.
- Right-click or Enter closes the polygon (needs ≥3 points) and calls
  `map.add_sector_from_polygon`; success clears the draft and reports
  "Sector N created", errors surface the rejection message.
- Escape clears the draft.

### `draw_wall_tool` (`tools.rs:151`)

- First left click snaps and stores the start point.
- Second left click resolves the start's sector via
  `find_sector_containing_or_on_edge` and calls `map.add_wall`; success/error is
  reported via `ToolState.message`.
- Right-click or Escape cancels the draft start.

### `obstacle_stamp_tool` (`tools.rs:190`)

- Press left to set the rectangle's first corner, drag to the second.
- On release, finds the sector at the rectangle's center
  (`map.find_sector_at`) and calls `map.add_obstacle`. Errors like "Obstacle
  must be inside a sector" are reported.

### `sync_obstacle_handles` + `move_dragged_obstacles`

- `sync_obstacle_handles` (`tools.rs:227`) — keeps one orange cube per obstacle
  at its centroid (2D mode), keyed by `(sector_id, obstacle_id)`.
- `move_dragged_obstacles` (`tools.rs:308`) — while an obstacle marker is
  dragged, translates that obstacle's pooled vertices so the whole box follows
  the cursor. The per-corner vertex indices are deduplicated (each corner is
  referenced by two edges), and the delta is computed non-cumulatively from the
  current center each frame so the box doesn't move at 2×/N× cursor speed.

### `select_click` (`tools.rs:345`)

Left click (Select tool, 2D): clears the previous selection, then checks the
hovered entity for a `VertexHandle` or `ObstacleHandle` (setting the
`Selected` tint marker), else selects the sector under the cursor
(`map.find_sector_at`). A release whose press moved <5 px counts as a click;
otherwise it was a drag and is ignored.

### `delete_selected` (`tools.rs:404`)

`Delete`/`Backspace` removes whatever is selected: a vertex
(`map.remove_vertex`), an obstacle (`map.remove_obstacle`), or a sector
(`map.remove_sector`), then resets the selection and reports the action.

### `nudge_selected_height` (`tools.rs:448`)

`[`/`]` nudges the selected sector's floor (or obstacle's bottom) by ±1;
`Shift+[` / `Shift+]` nudges the ceiling (or top). Heights stay clamped through
the sector/obstacle setters.

### `draw_tool_gizmos` (`tools.rs:500`)

Per-tool overlays (all ~0.02–0.05 above ground, pure gizmos):

- **Draw Sector**: green crosses at draft points, green preview lines between
  them and a rubber-band to the cursor.
- **Draw Wall**: a light-blue rubber-band line + start-point cross.
- **Place Obstacle**: an orange preview rectangle.
- **Select**: highlights the selected sector's wall outline (orange), or the
  selected obstacle's edges.

## 3D height editing (`height_handles.rs`)

`height_handles.rs` (747 lines) adds height editing in 3D: select a sector or
obstacle, then drag its two height handles, or grab an obstacle body directly.

### Components / resources

- `HeightHandle` (`height_handles.rs:26`) — an enum of the four editable
  bounds: `SectorFloor(id)`, `SectorCeiling(id)`, `ObstacleBottom { sector_id,
  obstacle_id }`, `ObstacleTop { sector_id, obstacle_id }`. Keys carry stable
  ids so they survive sector/obstacle reindexing.
- `ObstacleDrag { sector_id, obstacle_id, press_pos, dragging }`
  (`height_handles.rs:35`) — whole-obstacle vertical drag state.

### Ray-based picking (`pick_target`, `pick_obstacle`)

- `ray_hit_height(ray, y)` (`height_handles.rs:71`) — ray vs a horizontal plane.
- `pick_target` (`height_handles.rs:94`) — the nearest sector or obstacle hit,
  testing each sector's floor/ceiling and each obstacle's bottom/top planes,
  taking the closest `t`. Returns `PickResult::{Sector, Obstacle}`.
- `pick_obstacle` (`height_handles.rs:125`) — nearest obstacle whose *mid-height*
  plane is hit (used for the body drag).

### `select_3d` (`height_handles.rs:144`)

Left click (Select tool, 3D): clicking a height handle selects its owner;
otherwise `pick_target` under the cursor. Uses the same click-vs-drag threshold
logic as the 2D `select_click`.

### `sync_height_handles` (`height_handles.rs:263`)

Runs in 3D mode whenever `Map`, `Selection`, or `ModeState` changes. For the
current selection it spawns/positions/despawns exactly two handles:

- A **green** cube (`Cuboid 0.8×0.4×0.8`) at the lower bound (floor / obstacle
  bottom) on the sector/obstacle centroid.
- A **blue** cube at the upper bound (ceiling / obstacle top).

Handles are `VisibleIn3D` and pickable.

### `drag_height_handles_3d` (`height_handles.rs:374`)

Resizes the selected bound. The handle is axis-locked: instead of intersecting
the cursor ray with a plane (degenerate when the camera lies on that plane), it
solves for the world-Y that projects back onto the cursor's screen Y using a
4-step Newton solve (`solve_handle_screen_y`, `height_handles.rs:450`). On
release it snaps the height to the nearest integer (unless `Alt`).

### `drag_obstacle_3d` + `apply_obstacle_center`

- `drag_obstacle_3d` (`height_handles.rs:522`) — press on an obstacle's body
  (mid-height pick), then move it **strictly vertically**: the same XZ is
  re-applied with a new Y, so only `bottom`/`top` change (`apply_obstacle_center`,
  `height_handles.rs:478`). On release it snaps the center to the grid
  (`snap_obstacle_to_grid`, `height_handles.rs:504`). Height handles take
  priority over the body drag.
- The Newton solve gives "glued to cursor" behavior for vertical movement.

### `draw_3d_gizmos` (`height_handles.rs:613`)

Outlines the selected sector at its real floor (green) and ceiling (blue)
heights, or the selected obstacle's bottom/top outlines plus white corner
pillars between them.

## The 3D preview (`map_preview.rs`)

`update_3d_preview` (`map_preview.rs:21`) regenerates the 3D preview whenever
`Map` or `ModeState` changes (full regeneration: despawn everything, respawn).
When not in 3D mode it still despawns stale entities but spawns nothing.

### Per-texture materials

Mesh data is grouped into **buckets keyed by texture handle**
(`build_sector_meshes`, `map_preview.rs:157`). Each bucket gets a material from
a `material_cache: Local<HashMap<Handle<Image>, Handle<StandardMaterial>>>`
with `base_color_texture: Some(image)`, `base_color: WHITE`,
`perceptual_roughness: 0.9`, and `cull_mode: None` (double-sided). Preview
entities are tagged `VisibleIn3D` and `Pickable::IGNORE` so they never race with
2D picking or the despawn/respawn cycle.

### Mesh construction

`MeshData` (`map_preview.rs:79`) accumulates `positions`, `normals`, `uvs`, and
`indices`; `into_mesh` (`map_preview.rs:129`) packs them into a triangle-list
`Mesh`. Helpers: `add_polygon` (fan-triangulation, `map_preview.rs:89`),
`add_wall_quad` (`map_preview.rs:107`).

`build_sector_meshes` builds, per sector:

- **Floor** — outline polygon at `floor_height`, front face up (`Vec3::Y`).
- **Ceiling** — outline polygon at `ceiling_height` with the vertex order
  **reversed** and an up-facing normal. See [Inward-facing ceiling](#inward-facing-ceiling).
- **Wall quads** — one quad per solid wall, from `floor_height` to
  `ceiling_height`, using `interior_normal` (perpendicular to the wall pointing
  into the sector, `map_preview.rs:150`).
- **Portal steps** — for lines with a `back_side_def`, only the *owner* sector
  (the one with the **lower id**, `map_preview.rs:182`) renders, and only the
  floor/ceiling **step** regions where the two sectors' heights differ — the
  lower/upper texture slots between `floor_lo..floor_hi` and `ceil_lo..ceil_hi`.
  The middle of the doorway stays open.
- **Obstacles** — top cap (`Vec3::Y`), bottom cap (`Vec3::NEG_Y`), and side
  quads, each with its own texture.

UVs are scaled `world_pos * 0.1` (one texture tile per 10 units).

## The egui toolbar (`ui.rs`)

`UiPlugin` (`ui.rs:9`) adds `EguiPlugin`, disables bevy_egui's automatic primary
context creation (`auto_create_primary_context = false`, because
`ModePlugin::toggle_mode` moves the primary context between cameras), and runs
`editor_ui` on `EguiPrimaryContextPass` (so it follows whichever camera is
active).

`editor_ui` (`ui.rs:21`) shows a single "Editor" window:

- Mode label ("3D View (Tab for 2D)" / "2D Edit (Tab for 3D)").
- Tool radio buttons: Select, Draw Sector, Draw Wall, Obstacle.
- A per-tool hint line.
- A yellow reminder to press Tab when a draw tool is active in 3D.
- If a sector is selected: `DragValue`s for floor/ceiling heights (clamped
  floor ≤ ceiling after edits).
- If an obstacle is selected: `DragValue`s for bottom/top (clamped).
- A gray hint about the 3D height handles in 3D mode.
- The current `ToolState.message` (status/errors) in light blue.

Note the egui values write straight into `map.sectors[..]` — the same heights
the 3D handles and `[`/`]` nudging edit.

## Application wiring (`main.rs` / `editor.rs`)

`main.rs` configures the primary window (1920×1080, `AutoVsync`, title
"My Bevy App"), adds `DefaultPlugins` and `editor::EditorPlugin`, and declares
the module tree. `editor.rs` is the composition root — `EditorPlugin` adds all
feature plugins in one tuple (`editor.rs:18`).

## Design decisions

### Two cameras, one egui context

Both cameras exist permanently; `toggle_mode` flips `Camera.is_active` and
`Visibility`. Systems are gated with `in_mode(...)` run conditions so e.g. the
fly camera only runs in 3D and handle sync/drag only run in 2D. This keeps all
editor state (camera transforms, picking) alive across switches.

egui renders only for cameras with an `EguiContext`, so **both** cameras keep
one permanently. `toggle_mode` moves only `PrimaryEguiContext` +
`EguiMultipassSchedule` to the now-active camera and removes them from the
inactive one. Commands apply atomically at the next sync point, so no frame
ever sees two primary contexts. `EguiContext` itself is **never** added or
removed: bevy_egui's `WindowToEguiContextMap` is only cleaned in the next
PreUpdate, so add/remove makes `capture_pointer_input_system` (PostUpdate)
panic on a stale map entry.

Because the inactive camera still has a stale `EguiContext`, `toggle_mode` also
sets `EguiContextSettings.capture_pointer_input = false` on the losing camera.
If the inactive camera captured, its `EguiContext` would report it as a
full-screen pointer hit (cameras have no `Pickable`, so they block everything
below), which sits above the mesh hits in picking order — the vertex handles
would become unhoverable and undraggable.

`toggle_mode` also clears both drag resources and strips `BeingDragged`, so a
mid-drag Tab can't cause cross-mode ghost dragging.

### Indexed vertices + `add_vertex` dedup

Sharing edges/corners exactly is what makes portals and clean geometry
possible. All geometry is written through `add_vertex` (`map.rs:731`), which
returns an existing index for an identical `Vec2`. `rebuild_vertices`
(`map.rs:388`) re-dedups after deletions so removed geometry doesn't leave
orphan pooled vertices.

### Double-sided materials

Preview materials use `cull_mode: None`. In Bevy 0.19, back faces of a
double-sided material get their normals flipped in the shader, so interiors are
lit consistently even when you stand inside a sector. Winding still decides
which side is the "front" for texture orientation.

### Texture tiling

Preview UVs are scaled to world units — `uv = world_pos * 0.1`, one texture
tile per 10 units. The image loader defaults to `ClampToEdge`, which painted
only the first tile and clamped everything else to the border colour. The fix
loads every map texture with a **Repeat** sampler:

```rust
let repeat = |s: &mut ImageLoaderSettings| {
    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
};
asset_server.load_builder().with_settings(repeat).load("floor_texture.png")
```

(`map.rs:41`.) `load_builder().with_settings(...)` is the Bevy 0.19 replacement
for the deprecated `load_with_settings`.

### Inward-facing ceiling

A polygon's front face follows its winding, and viewing a double-sided polygon
from the back mirrors its texture. The ceiling is built with the outline
reversed so its front face points **down into the room** (unmirrored when seen
from inside), while keeping an **up-facing normal** so it still catches the top
light (`map_preview.rs:170`). Winding and normal are independent attributes, so
the ceiling reads correctly from inside *and* stays lit.

### Portal rendering

A portal line exists in both sectors' wall lists. Rendering it twice (as a
solid wall) would block the doorway; rendering it from both sectors would
double-draw. `build_sector_meshes` handles it by:

1. Only the sector with the **lower id** builds the portal (`map_preview.rs:182`).
2. It draws only the **step regions** — the vertical bands where the shared
   floor/ceiling heights differ (`floor_lo..floor_hi` and `ceil_lo..ceil_hi`) —
   using the lower/upper texture slots.

So a door between two sectors shows floor/ceiling steps but an open middle.

### Click vs drag disambiguation

Every select path (2D `select_click`, 3D `select_3d`, and the drag pipeline)
uses the same rule: a release within 5 px of the press counts as a click;
anything further is a drag that the drag systems already handled. This keeps
selection from fighting dragging.

### Snapping is everywhere

- Draw tools snap new points to existing vertices within `SNAP_RADIUS = 1.0`
  and to the 1-unit grid (`PickingState.ground_hit_snapped`).
- Dragged vertices/obstacles snap to the 1-unit grid on release unless `Alt`.
- 3D height handles snap to integer heights on release unless `Alt`.
- Freehand drawing without snapping is possible by placing points far from
  existing vertices, but exact-aligned clicks are what let portals form.

## The portal algorithm

This is the piece that makes drawing adjacent sectors feel automatic. The
relevant code lives in `map.rs` (`add_sector_from_polygon` `:77`, `add_wall`
`:190`, `collect_edge_overlaps` `:468`, `split_overlapped_walls` `:493`,
`collinear_overlap` `:616`, `find_wall_at_edge` `:452`).

**The invariant**: any edge of a new polygon/wall that overlaps an existing
*solid* wall — fully or partially — becomes a portal. Overlapping an existing
*portal* is a 3-way junction and is rejected before anything mutates.

The algorithm, for `add_sector_from_polygon`:

1. **Validate + normalize.** Check ≥3 points and non-zero signed area; wind the
   polygon CCW via `ensure_ccw`.
2. **Plan all overlaps up front.** For every edge, `collect_edge_overlaps`
   scans every existing wall and returns the overlap intervals (ordered along
   the new edge). Any hit on a wall that already has a `back_side_def` aborts
   with "An edge overlaps an existing portal (3-way junction is not
   supported)". Because this happens before any mutation, a rejected polygon
   leaves the map untouched.
3. **Split the overlapped walls.** `split_overlapped_walls` gathers interior
   cut parameters per (sector, wall), sorts/dedups them, and rebuilds each
   affected sector's wall list — splitting the overlapped wall into solid
   sub-walls at every boundary. Splitting all walls *before* building the new
   sector's walls means every portal piece has an exact-matching sub-wall to
   bind to.
4. **Build the new sector's walls piecewise.** Each new edge is divided at
   every overlap boundary that lies on it (collected via `perp_dot` collinearity
   + parameter `t`). For each piece, `find_wall_at_edge` checks for an exact
   vertex match against the (now split) existing walls; matching pieces become
   `portal_wall`s and are queued in `portal_pairs`; everything else is a
   `solid_wall`. New walls get fresh `WallId::new(id, index)` ids; the existing
   wall's own `WallId` was renumbered by the split.
5. **Convert the other side.** After the new sector exists, each queued
   `to_portal_wall` call upgrades the matching existing wall's `back_side_def`
   to point at the new sector.

`add_wall` mirrors this for a single edge against one existing sector.

Key helpers:

- `collinear_overlap(a, b, c, d)` (`map.rs:616`) — returns the overlap interval
  of segments `(a,b)` and `(c,d)` ordered along `(a,b)`, or `None` if they are
  not collinear/overlapping in positive length.
- `find_wall_at_edge(a, b)` (`map.rs:452`) — exact `(a,b)` or `(b,a)` vertex
  match. Because both sides write through `add_vertex`, matching pieces resolve
  to the exact same pooled indices, and the two portal walls end up traversing
  the edge in opposite directions over shared vertices.
- `EdgeOverlap` (`map.rs:641`) — `{ start, end, sector, wall }`.

The three overlap test cases in `map::tests` (`partial_overlap_splits_wall_into_portal`,
`edge_longer_than_wall_splits_new_edge`, `edge_spanning_two_walls_splits_both`)
exercise: a partial side overlap splitting an existing wall into three pieces,
an edge longer than the wall (overhang stays solid), and one edge spanning two
adjacent walls (splits both, two portals).

## Test coverage

Run with `cargo test`. All tests are pure unit tests (no window, no assets).

**`map::tests`** (`map.rs:1273`):
- `sector_from_polygon_builds_defaults` — defaults, wall count, `WallId`s.
- `sector_from_polygon_rejects_degenerate` — <3 points and zero-area polygons.
- `sector_from_polygon_normalizes_winding` — CCW normalization.
- `shared_edge_dedup_vertices` — two adjacent rects share 2 pooled vertices
  (8 → 6).
- `adjacent_sectors_auto_portal` — exact shared edge auto-portals both ways.
- `portal_shares_vertices_opposite_winding` — the two portal walls are
  `(start,end)` vs `(end,start)` over the same indices.
- `partial_overlap_splits_wall_into_portal` — partial overlap splits a wall
  into 3 pieces; the middle becomes a portal.
- `edge_longer_than_wall_splits_new_edge` — overhang stays solid, shared part
  portals.
- `edge_spanning_two_walls_splits_both` — one edge crossing a corner becomes
  two portal pieces facing both neighbors.
- `three_way_junction_rejected` — overlapping an existing portal errors and
  leaves the map unchanged.
- `remove_sector_scrubs_neighbor_portal` — deleting a sector un-portals the
  neighbor and reclaims shared vertices.
- `remove_vertex_cleans_walls_and_sectors` — deleting a shared corner collapses
  both sectors.
- `remove_obstacle` — removes obstacle and reclaims its vertices.
- `point_in_sector_and_find_sector_at` — containment + innermost-wins nesting.
- `snap_to_vertex_helper`.
- `sector_height_setters_clamp`, `obstacle_height_setters_clamp`.
- `point_in_polygon_and_obstacle_center`.

**`height_handles::tests`** (`height_handles.rs:645`):
- `pick_target_hits_floor_and_ceiling` — ray down hits floor, up hits ceiling.
- `pick_obstacle_hits_body` — mid-height pick.
- `apply_obstacle_center_translates_and_shifts_heights`.
- `obstacle_body_drag_is_vertical_only` — XZ never moves, only bottom/top.

## Recent change log

| Commit | What it introduced |
| ------ | ------------------ |
| `e1ea5ab` "Proud of myself" | First picking groundwork; scene/editor wiring. |
| `57572e4` "Much better" | Reworked `picking.rs` and `scene.rs` lighting setup. |
| `6b7dee5` "Getting to drag" | Split picking into `picking/` module (`state`, `controller`, `drag`, `visuals`). |
| `c21584d` "Drag not working" | Drag threshold + grab-offset work-in-progress. |
| `4a0285a` "DRAG WORKING" | Working vertex drag; removed click/highlight modules. |
| `d38b19d` "Beautiful" | Drag cleanup. |
| `e748145` "insane" | Full map data model, `mode.rs`, fly camera, gizmos, handles, and the first 3D preview (`map_preview.rs`). |
| `27662fc` "Fixed lighting" | Per-texture material buckets, Repeat-sampler tiling fix, inward-facing ceiling, portal step rendering. |
| `698118d` "Added guide" | Initial version of this guide. |
| `222d8cc` "Got drawing working" | The 2D draw tools: `tools.rs`, `draw_sector_tool`, `draw_wall_tool`, auto-portal on exact edges. |
| `3b6bdc7` "Fixed the obstacles" | Obstacle stamping + handles, `add_obstacle`, `move_dragged_obstacles`. |
| `4d51d32` "Added height modifying" | `height_handles.rs`: 3D height handles, obstacle body drag, `[`/`]` nudging, egui height values. |

The most recent commit (`4d51d32`) is the height-editing work. Following the
last commit, `add_sector_from_polygon`/`add_wall` gained **partial-overlap
portal detection** (an uncommitted change) — see [The portal algorithm](#the-portal-algorithm).

## Extending the editor

- **New map geometry / runtime operations**: add methods to `impl Map` in
  `map.rs` (modeled on `add_sector_from_polygon`, `add_obstacle`,
  `remove_vertex`). Follow the same discipline: validate everything up front so
  errors never mutate the map, write geometry through `add_vertex`, and keep
  `rebuild_vertices` in mind for deletions.
- **New textures**: assign per-face handles in `MapAssets` (`map.rs:32`) or in
  `test_map`; the per-texture buckets and `material_cache` pick them up
  automatically.
- **New 2D tooling**: model it on the draw tools — add an `EditorTool` variant,
  a draft resource, a system gated with
  `tool_is(EditorTool::X).and_then(in_mode(EditorMode::Edit2D))`, and surface
  feedback through `ToolState.message`. Read input positions from
  `PickingState.ground_hit_snapped` and snap with `snap_to_vertex`.
- **New 3D tooling**: add systems with `in_mode(EditorMode::View3D)` and read
  `PickingState.camera_ray`; model ray tests on `height_handles::pick_target`.
- **New selection targets**: extend the `Selection` resource and the
  select/delete systems in `tools.rs` (2D) and `height_handles.rs` (3D).
- **Height handling**: extend the `HeightHandle` enum + `set_target_y` /
  `handle_position` dispatch in `height_handles.rs`.
- **Preview geometry**: add cases in `build_sector_meshes` / `MeshData`
  (`map_preview.rs`).
- **Automated verification**: every behavioral rule added to `map.rs` should
  come with a unit test in `map::tests` (the overlap/portal tests are the model).
