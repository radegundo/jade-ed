# jade-ed — Architecture Guide

A walkthrough of how this codebase is put together. It explains every module, the
design decisions behind the recent changes, and where things plug in if you want
to extend the editor.

## Overview

`jade-ed` is a Bevy 0.19 map editor prototype. The map is a Doom-style sector
model (2D floor polygons with floor/ceiling heights) that you can:

- **Edit in 2D** — an orthographic top-down view where you drag vertex handles.
- **Walk through in 3D** — a fly camera inside the rendered sectors.

Press **Tab** to switch between the two modes.

```
src/
  main.rs             App + window + plugin registration
  editor.rs           EditorPlugin: lists all sub-plugins
  map.rs              Map data model + builders + the test map
  mode.rs             Mode state, camera spawning, 2D camera controls
  viewport.rs         3D fly camera + InfiniteGrid
  scene.rs            DirectionalLight
  map_handles.rs      Pickable vertex-handle spheres (2D)
  map_gizmos.rs       Wall/portal/vertex lines drawn in 2D
  map_preview.rs      3D preview: map data -> textured meshes/materials
  ui.rs               egui "Mode" window
  picking/            Mouse picking + drag system
```

Dependencies (`Cargo.toml`): `bevy` 0.19 (with `dynamic_linking`,
`bevy_dev_tools`), `bevy_egui` 0.41.1, `bevy-inspector-egui` 0.37.

## Running the project

```sh
cargo run
```

Run with `cargo run` from the project root. Launching the built binary directly
(`./target/debug/jade-ed`) fails because `dynamic_linking` requires the
`libbevy_dylib` shared library to be found.

### Controls

| Mode   | Action                     | Input                                   |
| ------ | -------------------------- | --------------------------------------- |
| any    | Toggle 3D / 2D             | `Tab`                                   |
| 3D     | Move                       | `W A S D` (ground-plane, FPS-style)     |
| 3D     | Fly up / down              | `E` / `Q`                               |
| 3D     | Look around                | Hold right mouse button                 |
| 3D     | Boost speed                | `Shift` or `Space`                      |
| 3D     | Move forward impulse       | Scroll wheel                            |
| 2D     | Pan                        | Middle or right mouse, or `Space` + left |
| 2D     | Zoom                       | Scroll wheel (smoothly eased)           |
| 2D     | Auto-pan at window edge    | Cursor near any window edge             |
| 2D     | Drag a vertex              | Left mouse on a handle (with 1-unit snap) |

## Module walkthrough

### `main.rs` and `editor.rs`

`main.rs` configures the primary window (1920×1080, `AutoVsync`) and adds
`DefaultPlugins` plus `editor::EditorPlugin`.

`editor.rs` is the composition root — a single `EditorPlugin` that adds all the
feature plugins:

```
ViewportPlugin  ScenePlugin  UiPlugin  OwnPickingPlugin
ModePlugin      MapPlugin    MapHandlesPlugin  MapGizmosPlugin  MapPreviewPlugin
```

### `map.rs` — the data model

The map is an **indexed vertex pool**: every `LineDef` stores `start_idx` /
`end_idx` into `Map.vertices` instead of raw positions. `add_vertex` de-duplicates
identical positions, so shared walls (portals) automatically share vertices
(`map.rs:85`).

Core types:

- `Map { vertices: Vec<Vec2>, sectors: Vec<Sector> }` — the `Resource`.
- `Sector { walls, obstacles, floor_height, ceiling_height, floor_texture,
  ceiling_texture, id }`.
- `LineDef { start_idx, end_idx, front_side_def, back_side_def, id: WallId }`.
  A `back_side_def` is what makes a line a **portal**.
- `SideDef { textures: SideDefTextures, facing }` with
  `SideDefTextures { upper, middle, lower }` — the classic lower/middle/upper
  texture slots.
- `Obstacle { id, edges, bottom, top, side_texture, top_texture, bottom_texture }`
  — a floating prism inside a sector.

Builders:

- `wall(...)` / `portal(...)` construct single `LineDef`s.
- `SectorBuilder` chains `.wall(...)`, `.portal(...)`, `.rect_obstacle(...)` then
  `.build()`.
- `rect_obstacle` builds a box; its edges are wound **clockwise** so the
  `interior_normal` helper produces **outward**-facing normals (`map.rs:239`).

Queries: `point_in_sector` (ray-cast point-in-polygon, `map.rs:417`) and
`find_player_sector`.

`test_map` (`map.rs:446`) builds the demo: two sectors sharing a portal, a box on
the floor, and a floating platform. **All textures are loaded with a Repeat
sampler** so the tiled UVs work — see "Texture tiling" below.

### `mode.rs` — modes and cameras

- `ModeState { mode }` resource + `EditorMode::{View3D, Edit2D}` is the single
  source of truth for the active mode.
- `Camera3D` / `Camera2D` tag the two cameras; `VisibleIn3D` / `VisibleIn2D` tag
  entities that should show in only one mode.
- `spawn_cameras` creates both cameras: the 3D perspective camera from the
  `FlyCamera` state, and a 2D orthographic camera (`ScalingMode::WindowSize`).
  The 2D camera starts hidden (`Visibility::Hidden`).
- `toggle_mode` (Tab) flips the mode, restores the cursor (so a locked 3D
  look-drag can't strand it hidden), toggles camera `is_active`/`Visibility`, and
  **clears drag state** so a mid-drag Tab can't cause cross-mode ghost dragging.
- `control_2d_camera` handles zoom (scroll sets a target scale, eased each
  frame), pan (middle/right/`Space`+left, with `1 pixel = scale world units` so
  content follows the cursor), and edge panning.
- `center_2d_camera_on_map` frames the map's bounding box once at startup.

### `viewport.rs` — the 3D fly camera

`FlyCamera` stores `position`, `yaw`, `pitch`, and speeds. The camera's `Transform`
is derived each frame:

```rust
transform.translation = fly.position;
transform.rotation = Quat::from_axis_angle(Vec3::Y, fly.yaw)
                   * Quat::from_axis_angle(Vec3::X, fly.pitch);
```

Key behavior (`control_fly_camera`):

- **FPS-style movement**: forward is horizontal-only —
  `Vec3::new(-sin(yaw), 0, -cos(yaw))` — so `W` never moves you into the sky,
  regardless of pitch.
- `Q`/`E` move straight up/down.
- Right mouse locks and hides the cursor while looking.
- Scroll wheel gives a one-shot forward nudge.
- The grid is Bevy's `InfiniteGrid` at y=0 with a 150-unit fadeout.

### `scene.rs`

A single `DirectionalLight` (illuminance 5000) shining down the `(-1,-2,-1)`
direction. This is the only light in the scene; the 3D preview relies on it plus
ambient light.

### `map_handles.rs` — vertex handles (2D)

- Spawns one green `Sphere` entity per map vertex (`VertexHandle { index }`),
  tagged `VisibleIn2D` and pickable.
- `sync_handles` reconciles handle entities with `Map.vertices`: updates
  positions, spawns missing handles, despawns removed ones. Entities that are
  `BeingDragged` are left alone (their transform is mid-drag).
- `sync_dragged_to_map` writes a dragged handle's X/Z back into `Map.vertices`
  so editing the 3D preview works from the 2D view.

### `map_gizmos.rs` — 2D overlay

Draws wall lines (gray, blue for portals — i.e. lines with a `back_side_def`) and
green vertex crosses each frame, slightly above the ground plane to avoid
z-fighting with the grid. These are pure `Gizmos`, no entities.

### `map_preview.rs` — the 3D preview

This is the largest and most recently reworked module. Every frame (when in
`View3D`) it turns `Map` data into textured meshes.

**Per-texture materials.** `update_3d_preview` keeps a
`material_cache: Local<HashMap<Handle<Image>, Handle<StandardMaterial>>>`. For
each sector it calls `build_sector_meshes`, which returns meshes grouped into
**buckets keyed by texture handle**. Each bucket gets a material with
`base_color_texture: Some(image)`, `base_color: WHITE`, `perceptual_roughness:
0.9`, and `cull_mode: None` (double-sided). Entities are tagged `VisibleIn3D`
and `Pickable::IGNORE` so they never race with 2D picking. Stale preview entities
are despawned before respawning (`map_preview.rs:37`).

**Mesh construction.** `build_sector_meshes` (`map_preview.rs:157`) builds:

- **Floor** — outline polygon at `floor_height`, front face up (`Vec3::Y`).
- **Ceiling** — outline polygon at `ceiling_height`, but with the vertex order
  **reversed** and an up-facing normal. See "Inward-facing ceiling" below.
- **Wall quads** — one quad per wall, using `interior_normal` (perpendicular to
  the wall, pointing into the sector).
- **Portal steps** — for lines with a `back_side_def`, only the *owner* sector
  (lower `id`) renders, and only the floor/ceiling *step* regions where the two
  sectors' heights differ. The middle of the doorway stays open.
- **Obstacles** — top/bottom caps plus side quads, each with its own texture.

`MeshData` accumulates `positions`, `normals`, `uvs`, and `indices`;
`into_mesh` packs them into a triangle-list `Mesh`.

### `ui.rs`

A minimal egui window showing the current mode ("3D View (Tab for 2D)" /
"2D Edit (Tab for 3D)").

### `picking/` — mouse picking and dragging

- `state.rs` — `PickingState`, the single per-frame snapshot of everything the
  pointer needs: camera ray, ground-plane hit (raw + snapped to the 1-unit
  grid), hovered entity + exact mesh hit point/normal, cursor position, button
  and modifier state. `snap_to_grid` snaps X/Z, preserving Y.
- `controller.rs` — `update_picking_state` runs in `PreUpdate` and fills
  `PickingState` using the active mode's camera (`viewport_to_world`) and the
  `MeshPickingPlugin` interaction results.
- `drag.rs` — `on_press` (observer) records a potential drag; `update_drag`
  waits until the cursor moves past a 5px threshold, computes a grab offset, then
  moves the entity each frame. On release it snaps to the grid (unless `Alt` is
  held). `Space`+left is reserved for panning, so it aborts any in-flight drag.
- `visuals.rs` — tints hovered/selected/dragged entities (blue / orange / yellow)
  without losing the original material, using a `MaterialCache` resource.
- `mod.rs` — `OwnPickingPlugin` wires the observers and systems, and gates
  `update_drag` to `Edit2D` mode.

## Design decisions

### Two cameras, one active

Both cameras exist permanently; `toggle_mode` flips `Camera.is_active` and
`Visibility`. Systems are gated with `in_mode(EditorMode::...)` run conditions so
e.g. the fly camera only runs in 3D and handle sync/drag only run in 2D. This
keeps all editor state (camera transforms, picking) alive across switches.

### Double-sided materials

Preview materials use `cull_mode: None`. In Bevy 0.19, back faces of a
double-sided material get their normals flipped in the shader
(`bevy_pbr/src/render/pbr_functions.wgsl`), so interiors are lit consistently
even when you stand inside a sector. Winding still decides which side is the
"front" for texture orientation.

### Texture tiling

Preview UVs are scaled to world units — `uv = world_pos * 0.1`, so one texture
tile every 10 units. The image loader defaults to `ClampToEdge`, which painted
only the first tile and clamped everything else to the texture's border colour.
The fix loads every map texture with a **Repeat** sampler:

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

(`map.rs:451`.) `load_builder().with_settings(...)` is the Bevy 0.19 replacement
for the deprecated `load_with_settings`.

### Inward-facing ceiling

A polygon's front face follows its winding, and viewing a double-sided polygon
from the back mirrors its texture. The ceiling is now built with the outline
reversed so its front face points **down into the room** (unmirrored when seen
from inside), while keeping an **up-facing normal** so it still catches the top
light (`map_preview.rs:170`). Winding and normal are independent attributes, so
the ceiling reads correctly from inside *and* stays lit.

### Portal rendering

A portal line exists in both sectors' wall lists. Rendering it twice (as a solid
wall) would block the doorway; rendering it from both sectors would double-draw.
`build_sector_meshes` handles it by:

1. Only the sector with the **lower id** builds the portal.
2. It draws only the **step regions** — the vertical bands where the shared
   floor/ceiling heights differ (`floor_lo..floor_hi` and `ceil_lo..ceil_hi`) —
   using the lower/middle/upper texture slots.

So a door between two sectors shows floor/ceiling steps but an open middle.

## Recent change log

The last several commits built and refined the 3D preview and the editing flow:

| Commit | What it introduced |
| ------ | ------------------ |
| `e1ea5ab` "Proud of myself" | First picking groundwork; scene/editor wiring. |
| `57572e4` "Much better" | Reworked `picking.rs` and `scene.rs` lighting setup. |
| `6b7dee5` "Getting to drag" | Split picking into `picking/` module (`state`, `controller`, `drag`, `visuals`). |
| `c21584d` "Drag not working" | Drag threshold + grab-offset work-in-progress. |
| `4a0285a` "DRAG WORKING" | Working vertex drag; removed click/highlight modules. |
| `d38b19d` "Beautiful" | Drag cleanup. |
| `e748145` "insane" | The big one: full map data model, `mode.rs`, fly camera, gizmos, handles, and the first 3D preview (`map_preview.rs`). |
| `27662fc` "Fixed lighting" | Per-texture material buckets, Repeat-sampler tiling fix, inward-facing ceiling, portal step rendering. |

The most recent commit (`27662fc`) is the texture/material work described in the
"Design decisions" section above.

## Extending

- **New map geometry**: add builder methods in `map.rs` (`SectorBuilder` /
  `ObstacleBuilder`) and surfaces in `build_sector_meshes`.
- **New textures**: assign per-face handles in `test_map` (or a loaded map); the
  buckets and `material_cache` pick them up automatically.
- **New 2D tooling**: model it on the drag pipeline — write to `PickingState`,
  register an observer/system in `picking/mod.rs`, gate it to `Edit2D`.
- **New 3D tooling**: add systems with `in_mode(EditorMode::View3D)` and use the
  `FlyCamera`/`Camera3D` query in `viewport.rs` or `map_preview.rs`.
