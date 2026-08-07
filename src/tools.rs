use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::map::{obstacle_center, snap_to_vertex, Map, MapAssets};
use crate::map_handles::VertexHandle;
use crate::mode::{in_mode, EditorMode, VisibleIn2D};
use crate::picking::drag::BeingDragged;
use crate::picking::state::PickingState;
use crate::picking::visuals::Selected;

/// How far a click may be from an existing vertex before the draw tools snap
/// to it. This is what makes shared corners/edges exact so portals can form.
const SNAP_RADIUS: f32 = 1.0;
const CLICK_DRAG_THRESHOLD: f32 = 5.0;

//------------------------------TOOL STATE---------------------------

#[derive(Resource, Default)]
pub struct ToolState {
    pub tool: EditorTool,
    /// Status/error message shown in the toolbar UI.
    pub message: Option<String>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub enum EditorTool {
    #[default]
    Select,
    DrawSector,
    DrawWall,
    PlaceObstacle,
}

/// Run condition: only run when the given tool is active.
pub fn tool_is(tool: EditorTool) -> impl FnMut(Res<ToolState>) -> bool + Clone {
    move |state: Res<ToolState>| state.tool == tool
}

//------------------------------DRAFT STATE--------------------------

/// In-progress freeform sector polygon (Draw Sector tool).
#[derive(Resource, Default)]
pub struct SectorDraft {
    pub points: Vec<Vec2>,
}

/// In-progress wall: the start point (Draw Wall tool).
#[derive(Resource, Default)]
pub struct WallDraft {
    pub start: Option<Vec2>,
}

/// In-progress obstacle rectangle (Place Obstacle tool).
#[derive(Resource, Default)]
pub struct ObstacleStamp {
    pub start: Option<Vec2>,
    pub current: Vec2,
}

/// Current selection: either an entity (vertex / obstacle / wall marker), a
/// sector index, an obstacle (sector id, obstacle id), or a wall (sector id,
/// wall index). Exactly one of the four should be set.
#[derive(Resource, Default)]
pub struct Selection {
    pub entity: Option<Entity>,
    pub sector: Option<usize>,
    pub obstacle: Option<(usize, usize)>,
    pub wall: Option<(usize, usize)>,
}

/// Pickable marker at an obstacle's centroid, for select / drag / delete.
#[derive(Component)]
pub struct ObstacleHandle {
    pub sector_id: usize,
    pub obstacle_id: usize,
}

/// Pickable marker along a wall, for select / texture assignment.
#[derive(Component)]
pub struct WallHandle {
    pub sector_id: usize,
    pub wall_index: usize,
}

//------------------------------PLUGIN-------------------------------

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolState>()
            .init_resource::<SectorDraft>()
            .init_resource::<WallDraft>()
            .init_resource::<ObstacleStamp>()
            .init_resource::<Selection>()
            .add_systems(
                Update,
                (
                    draw_sector_tool.run_if(tool_is(EditorTool::DrawSector).and_then(in_mode(EditorMode::Edit2D))),
                    draw_wall_tool.run_if(tool_is(EditorTool::DrawWall).and_then(in_mode(EditorMode::Edit2D))),
                    obstacle_stamp_tool.run_if(tool_is(EditorTool::PlaceObstacle).and_then(in_mode(EditorMode::Edit2D))),
                ),
            )
            .add_systems(
                Update,
                (
                    sync_obstacle_handles,
                    sync_wall_handles,
                    move_dragged_obstacles,
                    select_click.run_if(tool_is(EditorTool::Select)),
                    delete_selected,
                    nudge_selected_height,
                    draw_tool_gizmos,
                )
                    .run_if(in_mode(EditorMode::Edit2D)),
            );
    }
}

//------------------------------DRAW SECTOR--------------------------

fn draw_sector_tool(
    mut draft: ResMut<SectorDraft>,
    mut map: ResMut<Map>,
    assets: Res<MapAssets>,
    mut state: ResMut<ToolState>,
    picking: Res<PickingState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left)
        && let Some(hit) = picking.ground_hit_snapped
    {
        let pos = hit.xz();
        let pos = snap_to_vertex(&map.vertices, pos, SNAP_RADIUS).unwrap_or(pos);
        draft.points.push(pos);
    }

    if mouse.just_pressed(MouseButton::Right) || keyboard.just_pressed(KeyCode::Enter) {
        if draft.points.len() >= 3 {
            match map.add_sector_from_polygon(&draft.points, &assets) {
                Ok(id) => {
                    state.message = Some(format!("Sector {id} created"));
                    draft.points.clear();
                }
                Err(e) => state.message = Some(e),
            }
        } else {
            draft.points.clear();
        }
    }

    if keyboard.just_pressed(KeyCode::Escape) {
        draft.points.clear();
    }
}

//------------------------------DRAW WALL----------------------------

fn draw_wall_tool(
    mut draft: ResMut<WallDraft>,
    mut map: ResMut<Map>,
    assets: Res<MapAssets>,
    mut state: ResMut<ToolState>,
    picking: Res<PickingState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left)
        && let Some(hit) = picking.ground_hit_snapped
    {
        let pos = hit.xz();
        let pos = snap_to_vertex(&map.vertices, pos, SNAP_RADIUS).unwrap_or(pos);
        match draft.start.take() {
            None => draft.start = Some(pos),
            Some(start) => {
                let result = map
                    .find_sector_containing_or_on_edge(start, SNAP_RADIUS)
                    .ok_or_else(|| "Wall start must be on or inside a sector".to_string())
                    .and_then(|sector_idx| {
                        let sector_id = map.sectors[sector_idx].id;
                        map.add_wall(sector_id, start, pos, &assets)
                    });
                state.message = Some(match result {
                    Ok(()) => "Wall added".to_string(),
                    Err(e) => e,
                });
            }
        }
    }

    if mouse.just_pressed(MouseButton::Right) || keyboard.just_pressed(KeyCode::Escape) {
        draft.start = None;
    }
}

//------------------------------PLACE OBSTACLE-----------------------

fn obstacle_stamp_tool(
    mut stamp: ResMut<ObstacleStamp>,
    mut map: ResMut<Map>,
    assets: Res<MapAssets>,
    mut state: ResMut<ToolState>,
    picking: Res<PickingState>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    if mouse.just_pressed(MouseButton::Left)
        && let Some(hit) = picking.ground_hit_snapped
    {
        stamp.start = Some(hit.xz());
        stamp.current = hit.xz();
    } else if let Some(start) = stamp.start {
        if let Some(hit) = picking.ground_hit_snapped {
            stamp.current = hit.xz();
        }
        if mouse.just_released(MouseButton::Left) {
            let end = stamp.current;
            let center = (start + end) * 0.5;
            state.message = Some(match map.find_sector_at(center) {
                Some(sector_idx) => {
                    let sector_id = map.sectors[sector_idx].id;
                    match map.add_obstacle(sector_id, start.x, start.y, end.x, end.y, &assets) {
                        Ok(id) => format!("Obstacle {id} placed"),
                        Err(e) => e,
                    }
                }
                None => "Obstacle must be inside a sector".to_string(),
            });
            stamp.start = None;
        }
    }
}

//------------------------------OBSTACLE HANDLES---------------------

fn sync_obstacle_handles(
    mut commands: Commands,
    map: Res<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut handles: Query<(Entity, &ObstacleHandle, Option<&BeingDragged>, &mut Transform)>,
    mut cube_mesh: Local<Option<Handle<Mesh>>>,
    mut cube_material: Local<Option<Handle<StandardMaterial>>>,
) {
    let mesh = match cube_mesh.as_ref() {
        Some(h) => h.clone(),
        None => {
            let h = meshes.add(Cuboid::from_size(Vec3::splat(0.6)));
            *cube_mesh = Some(h.clone());
            h
        }
    };
    let material = match cube_material.as_ref() {
        Some(h) => h.clone(),
        None => {
            let h = materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.55, 0.1),
                unlit: true,
                ..default()
            });
            *cube_material = Some(h.clone());
            h
        }
    };

    let mut by_key: HashMap<(usize, usize), Entity> = HashMap::new();
    for (entity, handle, _, _) in &handles {
        by_key.insert((handle.sector_id, handle.obstacle_id), entity);
    }

    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for sector in &map.sectors {
        for obs in &sector.obstacles {
            let key = (sector.id, obs.id);
            seen.insert(key);
            let center = obstacle_center(obs, &map.vertices);
            let target = Vec3::new(center.x, 0.0, center.y);
            if let Some(&entity) = by_key.get(&key)
                && let Ok((_, _, dragged, mut transform)) = handles.get_mut(entity)
                && dragged.is_none()
                && transform.translation != target
            {
                transform.translation = target;
            }
        }
    }

    for sector in &map.sectors {
        for obs in &sector.obstacles {
            let key = (sector.id, obs.id);
            if !by_key.contains_key(&key) {
                let center = obstacle_center(obs, &map.vertices);
                commands.spawn((
                    ObstacleHandle { sector_id: sector.id, obstacle_id: obs.id },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(Vec3::new(center.x, 0.0, center.y)),
                    Pickable::default(),
                    VisibleIn2D,
                ));
            }
        }
    }

    for (key, &entity) in by_key.iter() {
        if !seen.contains(key) {
            commands.entity(entity).despawn();
        }
    }
}

//------------------------------WALL HANDLES-------------------------

/// Transform for a pickable wall-handle cuboid: local +X lies along the wall
/// from `start` to `end` in map coordinates (map y maps to world z).
/// `Quat::from_rotation_y(θ)` sends local +X to `(cosθ, 0, -sinθ)`, so the
/// angle must be `atan2(-Δy, Δx)` for the cuboid to point along the wall
/// instead of its mirror (which would flip diagonal walls in y).
fn wall_handle_transform(start: Vec2, end: Vec2) -> Transform {
    let mid = (start + end) * 0.5;
    let delta = end - start;
    Transform {
        translation: Vec3::new(mid.x, 0.1, mid.y),
        rotation: Quat::from_rotation_y((-delta.y).atan2(delta.x)),
        scale: Vec3::new(delta.length().max(0.001), 1.0, 1.0),
    }
}

/// Spawn/update a pickable thin cuboid along every wall. Portal edges are owned
/// by the lower-id sector (mirroring the 3D preview) so each shared edge gets
/// exactly one handle. Handles are not draggable — they exist for select and
/// texture assignment.
fn sync_wall_handles(
    mut commands: Commands,
    map: Res<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut handles: Query<(Entity, &WallHandle, &mut Transform)>,
    mut cuboid_mesh: Local<Option<Handle<Mesh>>>,
    mut cuboid_material: Local<Option<Handle<StandardMaterial>>>,
) {
    let mesh = match cuboid_mesh.as_ref() {
        Some(h) => h.clone(),
        None => {
            let h = meshes.add(Cuboid::new(1.0, 0.3, 0.4));
            *cuboid_mesh = Some(h.clone());
            h
        }
    };
    let material = match cuboid_material.as_ref() {
        Some(h) => h.clone(),
        None => {
            let h = materials.add(StandardMaterial {
                base_color: Color::srgba(0.9, 0.7, 0.2, 0.35),
                unlit: true,
                ..default()
            });
            *cuboid_material = Some(h.clone());
            h
        }
    };

    let mut by_key: HashMap<(usize, usize), Entity> = HashMap::new();
    for (entity, handle, _) in &handles {
        by_key.insert((handle.sector_id, handle.wall_index), entity);
    }

    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for sector in &map.sectors {
        for (windex, wall) in sector.walls.iter().enumerate() {
            // Only the lower-id sector owns the handle for a portal edge.
            if let Some(back) = &wall.back_side_def
                && sector.id >= back.facing
            {
                continue;
            }
            let key = (sector.id, windex);
            seen.insert(key);

            // Unit cuboid (local +X along the wall), scaled/rotated to match.
            let start = *wall.start(&map.vertices);
            let end = *wall.end(&map.vertices);
            let target = wall_handle_transform(start, end);

            match by_key.get(&key).copied() {
                Some(entity) => {
                    if let Ok((_, _, mut transform)) = handles.get_mut(entity)
                        && *transform != target
                    {
                        *transform = target;
                    }
                }
                None => {
                    commands.spawn((
                        WallHandle { sector_id: sector.id, wall_index: windex },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        target,
                        Pickable::default(),
                        VisibleIn2D,
                    ));
                }
            }
        }
    }

    for (key, &entity) in by_key.iter() {
        if !seen.contains(key) {
            commands.entity(entity).despawn();
        }
    }
}

/// While an obstacle marker is being dragged, translate that obstacle's pooled
/// vertices so the whole box follows the cursor. The marker tracks the cursor,
/// so each frame we shift the box so its center lands on the marker's current
/// position — this is non-cumulative (adding a since-drag-start delta would
/// accumulate and send the obstacle flying at N× cursor speed).
fn move_dragged_obstacles(
    mut map: ResMut<Map>,
    dragged: Query<(&Transform, &BeingDragged, &ObstacleHandle)>,
) {
    for (transform, _drag, handle) in &dragged {
        let Some(sector_index) = map.sectors.iter().position(|s| s.id == handle.sector_id) else {
            continue;
        };
        let Some(obs_index) = map.sectors[sector_index]
            .obstacles
            .iter()
            .position(|o| o.id == handle.obstacle_id)
        else {
            continue;
        };
        let (delta, idxs) = {
            let obs = &map.sectors[sector_index].obstacles[obs_index];
            let center = obstacle_center(obs, &map.vertices);
            let delta = transform.translation.xz() - center;
            // Each box corner is referenced by two edges (start of one, end of
            // the next), so dedup before moving: applying the delta twice per
            // corner translates the box 2× and makes it diverge.
            let mut idxs = HashSet::new();
            for edge in &obs.edges {
                idxs.insert(edge.start_idx);
                idxs.insert(edge.end_idx);
            }
            (delta, idxs)
        };
        for idx in idxs {
            map.vertices[idx] += delta;
        }
    }
}

//------------------------------SELECTION + DELETE-------------------

fn select_click(
    mut commands: Commands,
    mut selection: ResMut<Selection>,
    map: Res<Map>,
    picking: Res<PickingState>,
    mouse: Res<ButtonInput<MouseButton>>,
    vertex_handles: Query<(Entity, &VertexHandle)>,
    obstacle_handles: Query<(Entity, &ObstacleHandle)>,
    wall_handles: Query<(Entity, &WallHandle)>,
    mut press_pos: Local<Option<Vec2>>,
) {
    // Clicks over egui are UI clicks, never selections.
    if picking.pointer_over_egui {
        return;
    }

    if mouse.just_pressed(MouseButton::Left) {
        *press_pos = Some(picking.cursor_pos);
        return;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let was_click = match *press_pos {
        Some(p) => picking.cursor_pos.distance(p) < CLICK_DRAG_THRESHOLD,
        None => true,
    };
    *press_pos = None;
    if !was_click {
        return; // it was a drag, already handled
    }

    // Clear the previous selection (and its tint marker).
    if let Some(entity) = selection.entity.take()
        && let Ok(mut e) = commands.get_entity(entity)
    {
        e.remove::<Selected>();
    }
    selection.sector = None;
    selection.obstacle = None;
    selection.wall = None;

    if let Some(hovered) = picking.hovered_entity {
        if let Ok((entity, _)) = vertex_handles.get(hovered) {
            selection.entity = Some(entity);
            if let Ok(mut e) = commands.get_entity(entity) {
                e.insert(Selected);
            }
            return;
        }
        if let Ok((entity, handle)) = obstacle_handles.get(hovered) {
            selection.entity = Some(entity);
            selection.obstacle = Some((handle.sector_id, handle.obstacle_id));
            if let Ok(mut e) = commands.get_entity(entity) {
                e.insert(Selected);
            }
            return;
        }
        if let Ok((entity, handle)) = wall_handles.get(hovered) {
            selection.entity = Some(entity);
            selection.wall = Some((handle.sector_id, handle.wall_index));
            if let Ok(mut e) = commands.get_entity(entity) {
                e.insert(Selected);
            }
            return;
        }
    }

    // Nothing mesh-like was hit: select the sector under the cursor.
    if let Some(hit) = picking.ground_hit_snapped {
        selection.sector = map.find_sector_at(hit.xz());
    }
}

fn delete_selected(
    mut commands: Commands,
    mut map: ResMut<Map>,
    mut selection: ResMut<Selection>,
    mut state: ResMut<ToolState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    vertex_handles: Query<(Entity, &VertexHandle)>,
    obstacle_handles: Query<(Entity, &ObstacleHandle)>,
    wall_handles: Query<(Entity, &WallHandle)>,
) {
    if !keyboard.just_pressed(KeyCode::Delete) && !keyboard.just_pressed(KeyCode::Backspace) {
        return;
    }

    let mut message = None;
    if let Some(entity) = selection.entity {
        if let Ok((_, h)) = vertex_handles.get(entity) {
            map.remove_vertex(h.index);
            message = Some("Vertex deleted".to_string());
        } else if let Ok((_, h)) = obstacle_handles.get(entity) {
            map.remove_obstacle(h.sector_id, h.obstacle_id);
            message = Some("Obstacle deleted".to_string());
        } else if wall_handles.get(entity).is_ok() {
            message = Some("Wall deletion isn't supported yet".to_string());
        }
        if let Ok(mut e) = commands.get_entity(entity) {
            e.remove::<Selected>();
        }
    } else if let Some(sector_index) = selection.sector
        && sector_index < map.sectors.len()
    {
        let id = map.sectors[sector_index].id;
        map.remove_sector(id);
        message = Some("Sector deleted".to_string());
    }

    if let Some(m) = message {
        state.message = Some(m);
    }
    *selection = Selection::default();
}

//------------------------------HEIGHT NUDGE (2D)--------------------

/// `[` / `]` nudge the selected sector's floor (or obstacle's bottom);
/// `Shift+[` / `Shift+]` nudge the ceiling (or top). Heights stay clamped
/// through the sector/obstacle setters.
fn nudge_selected_height(
    mut map: ResMut<Map>,
    selection: Res<Selection>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    let is_upper =
        keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let step = if keyboard.just_pressed(KeyCode::BracketLeft) {
        Some(-1.0)
    } else if keyboard.just_pressed(KeyCode::BracketRight) {
        Some(1.0)
    } else {
        None
    };
    let Some(step) = step else {
        return;
    };

    if let Some(sector_index) = selection.sector
        && sector_index < map.sectors.len()
    {
        let sector = &mut map.sectors[sector_index];
        if is_upper {
            sector.set_ceiling_height(sector.ceiling_height + step);
        } else {
            sector.set_floor_height(sector.floor_height + step);
        }
        return;
    }

    if let Some((sector_id, obstacle_id)) = selection.obstacle {
        let Some(sector_index) = map.sectors.iter().position(|s| s.id == sector_id) else {
            return;
        };
        let Some(obs_index) = map.sectors[sector_index]
            .obstacles
            .iter()
            .position(|o| o.id == obstacle_id)
        else {
            return;
        };
        let obs = &mut map.sectors[sector_index].obstacles[obs_index];
        if is_upper {
            obs.set_top(obs.top + step);
        } else {
            obs.set_bottom(obs.bottom + step);
        }
    }
}

//------------------------------GIZMOS-------------------------------

fn draw_tool_gizmos(
    mut gizmos: Gizmos,
    tool: Res<ToolState>,
    sector_draft: Res<SectorDraft>,
    wall_draft: Res<WallDraft>,
    stamp: Res<ObstacleStamp>,
    selection: Res<Selection>,
    map: Res<Map>,
    picking: Res<PickingState>,
) {
    let cursor = picking.ground_hit_snapped.map(|v| v.xz());

    match tool.tool {
        EditorTool::DrawSector => {
            for p in sector_draft.points.iter() {
                let v = Vec3::new(p.x, 0.02, p.y);
                let s = 0.3;
                let color = Color::srgb(0.6, 1.0, 0.3);
                gizmos.line(v + Vec3::new(-s, 0.0, -s), v + Vec3::new(s, 0.0, s), color);
                gizmos.line(v + Vec3::new(-s, 0.0, s), v + Vec3::new(s, 0.0, -s), color);
            }
            if sector_draft.points.len() >= 2 {
                for w in sector_draft.points.windows(2) {
                    gizmos.line(
                        Vec3::new(w[0].x, 0.02, w[0].y),
                        Vec3::new(w[1].x, 0.02, w[1].y),
                        Color::srgb(0.6, 1.0, 0.3),
                    );
                }
            }
            if let (Some(last), Some(cursor)) = (sector_draft.points.last(), cursor) {
                gizmos.line(
                    Vec3::new(last.x, 0.02, last.y),
                    Vec3::new(cursor.x, 0.02, cursor.y),
                    Color::srgb(0.6, 1.0, 0.3),
                );
            }
        }
        EditorTool::DrawWall => {
            if let (Some(start), Some(cursor)) = (wall_draft.start, cursor) {
                gizmos.line(
                    Vec3::new(start.x, 0.02, start.y),
                    Vec3::new(cursor.x, 0.02, cursor.y),
                    Color::srgb(0.8, 0.8, 1.0),
                );
                let v = Vec3::new(start.x, 0.02, start.y);
                let s = 0.3;
                let color = Color::srgb(0.8, 0.8, 1.0);
                gizmos.line(v + Vec3::new(-s, 0.0, -s), v + Vec3::new(s, 0.0, s), color);
                gizmos.line(v + Vec3::new(-s, 0.0, s), v + Vec3::new(s, 0.0, -s), color);
            }
        }
        EditorTool::PlaceObstacle => {
            if let Some(start) = stamp.start {
                let end = stamp.current;
                let a = Vec3::new(start.x, 0.02, start.y);
                let b = Vec3::new(end.x, 0.02, start.y);
                let c = Vec3::new(end.x, 0.02, end.y);
                let d = Vec3::new(start.x, 0.02, end.y);
                let color = Color::srgb(1.0, 0.55, 0.1);
                gizmos.line(a, b, color);
                gizmos.line(b, c, color);
                gizmos.line(c, d, color);
                gizmos.line(d, a, color);
            }
        }
        EditorTool::Select => {
            if let Some(sector_index) = selection.sector
                && sector_index < map.sectors.len()
            {
                for w in &map.sectors[sector_index].walls {
                    let s = *w.start(&map.vertices);
                    let e = *w.end(&map.vertices);
                    gizmos.line(
                        Vec3::new(s.x, 0.05, s.y),
                        Vec3::new(e.x, 0.05, e.y),
                        Color::srgb(1.0, 0.6, 0.1),
                    );
                }
            }
            if let Some((sector_id, obstacle_id)) = selection.obstacle
                && let Some(sector_index) = map.sectors.iter().position(|s| s.id == sector_id)
                && let Some(obs) = map.sectors[sector_index]
                    .obstacles
                    .iter()
                    .find(|o| o.id == obstacle_id)
            {
                for e in &obs.edges {
                    let s = *e.start(&map.vertices);
                    let e = *e.end(&map.vertices);
                    gizmos.line(
                        Vec3::new(s.x, 0.05, s.y),
                        Vec3::new(e.x, 0.05, e.y),
                        Color::srgb(1.0, 0.6, 0.1),
                    );
                }
            }
            if let Some((sector_id, wall_index)) = selection.wall
                && let Some(sector_index) = map.sectors.iter().position(|s| s.id == sector_id)
                && wall_index < map.sectors[sector_index].walls.len()
            {
                let wall = &map.sectors[sector_index].walls[wall_index];
                let s = *wall.start(&map.vertices);
                let e = *wall.end(&map.vertices);
                gizmos.line(
                    Vec3::new(s.x, 0.06, s.y),
                    Vec3::new(e.x, 0.06, e.y),
                    Color::srgb(1.0, 0.6, 0.1),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_handle_aligns_with_wall_direction() {
        for (start, end) in [
            (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),  // horizontal
            (Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0)),  // vertical
            (Vec2::new(0.0, 0.0), Vec2::new(10.0, 5.0)),  // diagonal
            (Vec2::new(2.0, 7.0), Vec2::new(-3.0, -4.0)), // reversed diagonal
        ] {
            let t = wall_handle_transform(start, end);
            let axis = t.rotation * Vec3::X;
            let want = Vec3::new(end.x - start.x, 0.0, end.y - start.y).normalize();
            assert!(
                (axis - want).length() < 1e-3,
                "handle axis {axis:?} must point along wall {want:?}"
            );
        }
    }

    #[test]
    fn wall_handle_spans_the_wall_length() {
        let t = wall_handle_transform(Vec2::new(0.0, 0.0), Vec2::new(3.0, 4.0));
        assert!((t.scale.x - 5.0).abs() < 1e-3, "scale.x must equal wall length");
        assert!((t.translation - Vec3::new(1.5, 0.1, 2.0)).length() < 1e-3);
    }
}
