//! 3D height editing: sector floor/ceiling and obstacle bottom/top.
//!
//! - Click a sector's floor/ceiling (or an obstacle's body) in 3D to select it.
//! - The selected target gets two draggable height handles (green = lower bound,
//!   blue = upper bound) that resize it vertically.
//! - An obstacle's body can be grabbed and dragged in full 3D (translate),
//!   moving its bottom and top together.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::map::{obstacle_center, point_in_polygon, Map, Obstacle};
use crate::mode::{in_mode, Camera3D, EditorMode, ModeState, VisibleIn3D};
use crate::picking::drag::{BeingDragged, DragState};
use crate::picking::state::PickingState;
use crate::tools::{EditorTool, Selection, ToolState};

const DRAG_THRESHOLD: f32 = 5.0;
const CLICK_DRAG_THRESHOLD: f32 = 5.0;

//------------------------------COMPONENTS / RESOURCES----------------

/// Pickable handle entity targeting one height bound of a sector/obstacle.
/// Keys carry the stable sector/obstacle *ids* so they survive reindexing.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum HeightHandle {
    SectorFloor(usize),
    SectorCeiling(usize),
    ObstacleBottom { sector_id: usize, obstacle_id: usize },
    ObstacleTop { sector_id: usize, obstacle_id: usize },
}

/// In-progress whole-obstacle vertical drag (3D).
#[derive(Resource, Default)]
pub struct ObstacleDrag {
    pub sector_id: Option<usize>,
    pub obstacle_id: Option<usize>,
    pub press_pos: Vec2,
    pub dragging: bool,
}

//------------------------------PLUGIN--------------------------------

pub struct HeightHandlesPlugin;

impl Plugin for HeightHandlesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ObstacleDrag>()
            .add_systems(Update, sync_height_handles.run_if(in_mode(EditorMode::View3D)))
            .add_systems(
                Update,
                (
                    select_3d.run_if(in_mode(EditorMode::View3D).and_then(|s: Res<ToolState>| s.tool == EditorTool::Select)),
                    drag_height_handles_3d.run_if(in_mode(EditorMode::View3D).and_then(|s: Res<ToolState>| s.tool == EditorTool::Select)),
                    drag_obstacle_3d.run_if(in_mode(EditorMode::View3D).and_then(|s: Res<ToolState>| s.tool == EditorTool::Select)),
                    draw_3d_gizmos.run_if(in_mode(EditorMode::View3D)),
                ),
            );
    }
}

//------------------------------RAYCAST PICKING-----------------------

#[derive(Debug, PartialEq, Clone, Copy)]
enum PickResult {
    Sector(usize),
    Obstacle { sector_id: usize, obstacle_id: usize },
}

/// Ray vs horizontal plane at height `y`. Returns (t, hit).
fn ray_hit_height(ray: &Ray3d, y: f32) -> Option<(f32, Vec3)> {
    if ray.direction.y.abs() < 1e-6 {
        return None;
    }
    let t = (y - ray.origin.y) / ray.direction.y;
    if t <= 0.0 {
        return None;
    }
    Some((t, ray.origin + ray.direction * t))
}

/// Ray vs a plane through `p0` with the given `normal`. Returns the hit point.
fn sector_index_by_id(map: &Map, id: usize) -> Option<usize> {
    map.sectors.iter().position(|s| s.id == id)
}

fn obstacle_ref(map: &Map, sector_id: usize, obstacle_id: usize) -> Option<(usize, &Obstacle)> {
    let si = sector_index_by_id(map, sector_id)?;
    let oi = map.sectors[si].obstacles.iter().position(|o| o.id == obstacle_id)?;
    Some((si, &map.sectors[si].obstacles[oi]))
}

/// Nearest sector or obstacle the ray hits (floors, ceilings, obstacle tops/bottoms).
fn pick_target(ray: &Ray3d, map: &Map) -> Option<PickResult> {
    let mut best: Option<(f32, PickResult)> = None;
    for (si, sector) in map.sectors.iter().enumerate() {
        let outline: Vec<Vec2> = sector.walls.iter().map(|w| *w.start(&map.vertices)).collect();
        for y in [sector.floor_height, sector.ceiling_height] {
            if let Some((t, hit)) = ray_hit_height(ray, y)
                && point_in_polygon(hit.xz(), &outline)
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, PickResult::Sector(si)));
            }
        }
        for obs in &sector.obstacles {
            let pts: Vec<Vec2> = obs.edges.iter().map(|e| *e.start(&map.vertices)).collect();
            for y in [obs.bottom, obs.top] {
                if let Some((t, hit)) = ray_hit_height(ray, y)
                    && point_in_polygon(hit.xz(), &pts)
                    && best.is_none_or(|(bt, _)| t < bt)
                {
                    best = Some((
                        t,
                        PickResult::Obstacle { sector_id: sector.id, obstacle_id: obs.id },
                    ));
                }
            }
        }
    }
    best.map(|(_, r)| r)
}

/// Nearest obstacle whose body (mid-height plane inside its polygon) the ray hits.
fn pick_obstacle(ray: &Ray3d, map: &Map) -> Option<(usize, usize)> {
    let mut best: Option<(f32, (usize, usize))> = None;
    for sector in &map.sectors {
        for obs in &sector.obstacles {
            let pts: Vec<Vec2> = obs.edges.iter().map(|e| *e.start(&map.vertices)).collect();
            let mid = (obs.bottom + obs.top) * 0.5;
            if let Some((t, hit)) = ray_hit_height(ray, mid)
                && point_in_polygon(hit.xz(), &pts)
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, (sector.id, obs.id)));
            }
        }
    }
    best.map(|(_, k)| k)
}

//------------------------------SELECTION (3D)------------------------

fn select_3d(
    mut commands: Commands,
    mut selection: ResMut<Selection>,
    map: Res<Map>,
    state: Res<PickingState>,
    mouse: Res<ButtonInput<MouseButton>>,
    handles: Query<(Entity, &HeightHandle)>,
    mut press_pos: Local<Option<Vec2>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        *press_pos = Some(state.cursor_pos);
        return;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let was_click = match *press_pos {
        Some(p) => state.cursor_pos.distance(p) < CLICK_DRAG_THRESHOLD,
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
        e.remove::<crate::picking::visuals::Selected>();
    }
    selection.sector = None;
    selection.obstacle = None;

    // Clicking a height handle selects its owner.
    if let Some(hovered) = state.hovered_entity
        && let Ok((_, handle)) = handles.get(hovered)
    {
        match *handle {
            HeightHandle::SectorFloor(id) | HeightHandle::SectorCeiling(id) => {
                selection.sector = sector_index_by_id(&map, id);
            }
            HeightHandle::ObstacleBottom { sector_id, obstacle_id }
            | HeightHandle::ObstacleTop { sector_id, obstacle_id } => {
                selection.obstacle = Some((sector_id, obstacle_id));
            }
        }
        return;
    }

    if let Some(ray) = state.camera_ray.as_ref()
        && let Some(result) = pick_target(ray, &map)
    {
        match result {
            PickResult::Sector(idx) => selection.sector = Some(idx),
            PickResult::Obstacle { sector_id, obstacle_id } => {
                selection.obstacle = Some((sector_id, obstacle_id))
            }
        }
    }
}

//------------------------------HANDLE SYNC---------------------------

fn handle_position(handle: &HeightHandle, map: &Map) -> Option<Vec3> {
    match *handle {
        HeightHandle::SectorFloor(id) | HeightHandle::SectorCeiling(id) => {
            let si = sector_index_by_id(map, id)?;
            let c = map.sector_centroid(si)?;
            let y = match *handle {
                HeightHandle::SectorFloor(_) => map.sectors[si].floor_height,
                _ => map.sectors[si].ceiling_height,
            };
            Some(Vec3::new(c.x, y, c.y))
        }
        HeightHandle::ObstacleBottom { sector_id, obstacle_id }
        | HeightHandle::ObstacleTop { sector_id, obstacle_id } => {
            let (_, obs) = obstacle_ref(map, sector_id, obstacle_id)?;
            let c = obstacle_center(obs, &map.vertices);
            let y = match *handle {
                HeightHandle::ObstacleBottom { .. } => obs.bottom,
                _ => obs.top,
            };
            Some(Vec3::new(c.x, y, c.y))
        }
    }
}

fn set_target_y(handle: &HeightHandle, map: &mut Map, y: f32) {
    match *handle {
        HeightHandle::SectorFloor(id) => {
            if let Some(i) = sector_index_by_id(map, id) {
                map.sectors[i].set_floor_height(y);
            }
        }
        HeightHandle::SectorCeiling(id) => {
            if let Some(i) = sector_index_by_id(map, id) {
                map.sectors[i].set_ceiling_height(y);
            }
        }
        HeightHandle::ObstacleBottom { sector_id, obstacle_id } => {
            if let Some(si) = sector_index_by_id(map, sector_id)
                && let Some(oi) = map.sectors[si].obstacles.iter().position(|o| o.id == obstacle_id)
            {
                map.sectors[si].obstacles[oi].set_bottom(y);
            }
        }
        HeightHandle::ObstacleTop { sector_id, obstacle_id } => {
            if let Some(si) = sector_index_by_id(map, sector_id)
                && let Some(oi) = map.sectors[si].obstacles.iter().position(|o| o.id == obstacle_id)
            {
                map.sectors[si].obstacles[oi].set_top(y);
            }
        }
    }
}

/// Spawn/position/despawn the two height handles for the active selection.
/// Runs in 3D mode only; handles carry `VisibleIn3D`.
fn sync_height_handles(
    mut commands: Commands,
    mut selection: ResMut<Selection>,
    map: Res<Map>,
    mode: Res<ModeState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut handles: Query<(Entity, &HeightHandle, &mut Transform)>,
    mut cube_mesh: Local<Option<Handle<Mesh>>>,
    mut floor_mat: Local<Option<Handle<StandardMaterial>>>,
    mut ceiling_mat: Local<Option<Handle<StandardMaterial>>>,
) {
    if !map.is_changed() && !selection.is_changed() && !mode.is_changed() {
        return;
    }

    let mesh = cube_mesh
        .get_or_insert_with(|| meshes.add(Cuboid::from_size(Vec3::new(0.8, 0.4, 0.8))))
        .clone();
    let floor_mat = floor_mat
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.3, 1.0, 0.3),
                unlit: true,
                ..default()
            })
        })
        .clone();
    let ceiling_mat = ceiling_mat
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.3, 0.5, 1.0),
                unlit: true,
                ..default()
            })
        })
        .clone();

    let mut by_key: HashMap<HeightHandle, Entity> = HashMap::new();
    for (entity, handle, _) in &handles {
        by_key.insert(*handle, entity);
    }

    let mut desired: Vec<(HeightHandle, Vec3, Handle<StandardMaterial>)> = Vec::new();
    if let Some(idx) = selection.sector {
        if idx < map.sectors.len() {
            let id = map.sectors[idx].id;
            if let Some(c) = map.sector_centroid(idx) {
                desired.push((
                    HeightHandle::SectorFloor(id),
                    Vec3::new(c.x, map.sectors[idx].floor_height, c.y),
                    floor_mat.clone(),
                ));
                desired.push((
                    HeightHandle::SectorCeiling(id),
                    Vec3::new(c.x, map.sectors[idx].ceiling_height, c.y),
                    ceiling_mat.clone(),
                ));
            }
        } else {
            selection.sector = None;
        }
    } else if let Some((sid, oid)) = selection.obstacle {
        if let Some((_, obs)) = obstacle_ref(&map, sid, oid) {
            let c = obstacle_center(obs, &map.vertices);
            desired.push((
                HeightHandle::ObstacleBottom { sector_id: sid, obstacle_id: oid },
                Vec3::new(c.x, obs.bottom, c.y),
                floor_mat.clone(),
            ));
            desired.push((
                HeightHandle::ObstacleTop { sector_id: sid, obstacle_id: oid },
                Vec3::new(c.x, obs.top, c.y),
                ceiling_mat.clone(),
            ));
        } else {
            selection.obstacle = None;
        }
    }

    for (handle, pos, mat) in &desired {
        if let Some(&entity) = by_key.get(handle) {
            if let Ok((_, _, mut transform)) = handles.get_mut(entity)
                && transform.translation != *pos
            {
                transform.translation = *pos;
            }
        } else {
            commands.spawn((
                *handle,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(*pos),
                Pickable::default(),
                VisibleIn3D,
            ));
        }
    }

    let seen: HashSet<HeightHandle> = desired.iter().map(|(h, _, _)| *h).collect();
    for (key, &entity) in by_key.iter() {
        if !seen.contains(key) {
            commands.entity(entity).despawn();
        }
    }
}

//------------------------------HEIGHT HANDLE DRAG--------------------

/// Vertical resize of the selected sector/obstacle bound. The handle tracks
/// the cursor's screen Y, mapped to a world Y via the camera projection.
fn drag_height_handles_3d(
    mut commands: Commands,
    mut drag_state: ResMut<DragState>,
    state: Res<PickingState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut map: ResMut<Map>,
    handles: Query<(Entity, &HeightHandle)>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3D>>,
    tool: Res<ToolState>,
) {
    if tool.tool != EditorTool::Select {
        if let Some(entity) = drag_state.entity
            && let Ok(mut ec) = commands.get_entity(entity)
        {
            ec.remove::<BeingDragged>();
        }
        *drag_state = DragState::default();
        return;
    }
    let Some(entity) = drag_state.entity else {
        return;
    };
    let Ok((_, handle)) = handles.get(entity) else {
        *drag_state = DragState::default();
        return;
    };

    // Released: final snap, then clean up.
    if !mouse.pressed(MouseButton::Left) {
        if drag_state.is_dragging {
            if let Some(y) = handle_position(handle, &map).map(|p| p.y) {
                let snapped = if state.alt_held { y } else { y.round() };
                set_target_y(handle, &mut map, snapped);
            }
            if let Ok(mut ec) = commands.get_entity(entity) {
                ec.remove::<BeingDragged>();
            }
        }
        *drag_state = DragState::default();
        return;
    }

    // Threshold: start the drag (adds the drag tint).
    if !drag_state.is_dragging {
        let delta = state.cursor_pos - drag_state.press_screen_pos;
        if delta.length() < DRAG_THRESHOLD {
            return;
        }
        drag_state.is_dragging = true;
        if let Ok(mut ec) = commands.get_entity(entity) {
            ec.insert(BeingDragged {
                grab_offset: Vec3::ZERO,
                start_position: handle_position(handle, &map).unwrap_or_default(),
            });
        }
    }

    let Some(current_pos) = handle_position(handle, &map) else {
        *drag_state = DragState::default();
        return;
    };
    let Ok((cam, cam_t)) = camera.single() else {
        return;
    };

    // Axis-locked vertical drag. Instead of intersecting the cursor ray with a
    // plane through the handle (which is degenerate when the camera lies exactly
    // on that plane), solve for the world-Y that projects back onto the cursor's
    // screen Y. This keeps the handle glued to the cursor in screen space.
    let target_y = solve_handle_screen_y(cam, cam_t, current_pos, state.cursor_pos.y);
    set_target_y(handle, &mut map, target_y);
}

/// Find the world Y (same x/z as `pos`) that projects to `target_screen_y`.
/// Iterative Newton solve using the camera projection; converges in a couple
/// of steps because the projection is (locally) monotonic in Y.
fn solve_handle_screen_y(
    cam: &Camera,
    cam_t: &GlobalTransform,
    pos: Vec3,
    target_screen_y: f32,
) -> f32 {
    let eps = 0.25;
    let mut y = pos.y;
    for _ in 0..4 {
        let Ok(now) = cam.world_to_viewport(cam_t, Vec3::new(pos.x, y, pos.z)) else {
            break;
        };
        let Ok(up) = cam.world_to_viewport(cam_t, Vec3::new(pos.x, y + eps, pos.z)) else {
            break;
        };
        // Signed screen-px per world-Y unit (negative: up on screen = lower y).
        let screen_per_world = (up.y - now.y) / eps;
        if screen_per_world.abs() < 1e-4 {
            break;
        }
        let dy = (target_screen_y - now.y) / screen_per_world;
        y += dy.clamp(-256.0, 256.0);
    }
    y
}

//------------------------------OBSTACLE BODY DRAG--------------------

fn apply_obstacle_center(map: &mut Map, sector_id: usize, obstacle_id: usize, target: Vec3) {
    let Some(si) = sector_index_by_id(map, sector_id) else {
        return;
    };
    let Some(oi) = map.sectors[si].obstacles.iter().position(|o| o.id == obstacle_id) else {
        return;
    };
    let center = obstacle_center(&map.sectors[si].obstacles[oi], &map.vertices);
    let dx = target.x - center.x;
    let dz = target.z - center.y;
    let dy = target.y - map.sectors[si].obstacles[oi].bottom;

    let mut idxs: HashSet<usize> = HashSet::new();
    for e in &map.sectors[si].obstacles[oi].edges {
        idxs.insert(e.start_idx);
        idxs.insert(e.end_idx);
    }
    let di = Vec2::new(dx, dz);
    for idx in idxs {
        map.vertices[idx] += di;
    }
    let obs = &mut map.sectors[si].obstacles[oi];
    obs.bottom += dy;
    obs.top += dy;
}

fn snap_obstacle_to_grid(map: &mut Map, sector_id: usize, obstacle_id: usize) {
    let Some(si) = sector_index_by_id(map, sector_id) else {
        return;
    };
    let Some(oi) = map.sectors[si].obstacles.iter().position(|o| o.id == obstacle_id) else {
        return;
    };
    let center = obstacle_center(&map.sectors[si].obstacles[oi], &map.vertices);
    let target = Vec3::new(
        center.x.round(),
        map.sectors[si].obstacles[oi].bottom.round(),
        center.y.round(),
    );
    apply_obstacle_center(map, sector_id, obstacle_id, target);
}

/// Grab an obstacle's body (raycast on press) and move it strictly vertically:
/// only the bottom/top heights change; XZ stays put.
fn drag_obstacle_3d(
    mut drag: ResMut<ObstacleDrag>,
    state: Res<PickingState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut map: ResMut<Map>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3D>>,
    handles: Query<&HeightHandle>,
    tool: Res<ToolState>,
) {
    if tool.tool != EditorTool::Select {
        *drag = ObstacleDrag::default();
        return;
    }

    if mouse.just_pressed(MouseButton::Left) {
        // Height handles take priority over the body drag.
        if let Some(hovered) = state.hovered_entity
            && handles.get(hovered).is_ok()
        {
            *drag = ObstacleDrag::default();
            return;
        }
        let picked = state
            .camera_ray
            .as_ref()
            .and_then(|ray| pick_obstacle(ray, &map));
        match picked {
            Some((sid, oid)) => {
                *drag = ObstacleDrag {
                    sector_id: Some(sid),
                    obstacle_id: Some(oid),
                    press_pos: state.cursor_pos,
                    dragging: false,
                };
            }
            None => *drag = ObstacleDrag::default(),
        }
        return;
    }

    let (Some(sector_id), Some(obstacle_id)) = (drag.sector_id, drag.obstacle_id) else {
        return;
    };
    if !mouse.pressed(MouseButton::Left) {
        if drag.dragging && !state.alt_held {
            snap_obstacle_to_grid(&mut map, sector_id, obstacle_id);
        }
        *drag = ObstacleDrag::default();
        return;
    }

    if !drag.dragging {
        let delta = state.cursor_pos - drag.press_pos;
        if delta.length() < DRAG_THRESHOLD {
            return;
        }
        drag.dragging = true;
    }

    let Ok((cam, cam_t)) = camera.single() else {
        return;
    };
    let Some(center) = obstacle_ref(&map, sector_id, obstacle_id).map(|(_, o)| {
        let c = obstacle_center(o, &map.vertices);
        Vec3::new(c.x, (o.bottom + o.top) * 0.5, c.y)
    }) else {
        *drag = ObstacleDrag::default();
        return;
    };
    // Solve for the world Y that projects back onto the cursor's screen Y,
    // then re-apply with the same XZ so the obstacle only moves vertically.
    let target_y = solve_handle_screen_y(cam, cam_t, center, state.cursor_pos.y);
    apply_obstacle_center(&mut map, sector_id, obstacle_id, Vec3::new(center.x, target_y, center.z));
}

//------------------------------GIZMOS--------------------------------

fn draw_polygon(gizmos: &mut Gizmos, pts: &[Vec2], y: f32, color: Color) {
    for w in pts.windows(2) {
        gizmos.line(
            Vec3::new(w[0].x, y, w[0].y),
            Vec3::new(w[1].x, y, w[1].y),
            color,
        );
    }
    if let (Some(&a), Some(&b)) = (pts.first(), pts.last()) {
        gizmos.line(Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y), color);
    }
}

/// Outline the selected sector/obstacle at its real heights in 3D.
fn draw_3d_gizmos(mut gizmos: Gizmos, selection: Res<Selection>, map: Res<Map>) {
    let outline = |sector: &crate::map::Sector| -> Vec<Vec2> {
        sector.walls.iter().map(|w| *w.start(&map.vertices)).collect()
    };

    if let Some(idx) = selection.sector
        && idx < map.sectors.len()
    {
        let s = &map.sectors[idx];
        let pts = outline(s);
        draw_polygon(&mut gizmos, &pts, s.floor_height, Color::srgb(0.3, 1.0, 0.3));
        draw_polygon(&mut gizmos, &pts, s.ceiling_height, Color::srgb(0.3, 0.5, 1.0));
    }

    if let Some((sid, oid)) = selection.obstacle
        && let Some((_, obs)) = obstacle_ref(&map, sid, oid)
    {
        let pts: Vec<Vec2> = obs.edges.iter().map(|e| *e.start(&map.vertices)).collect();
        draw_polygon(&mut gizmos, &pts, obs.bottom, Color::srgb(0.3, 1.0, 0.3));
        draw_polygon(&mut gizmos, &pts, obs.top, Color::srgb(0.3, 0.5, 1.0));
        for p in &pts {
            gizmos.line(
                Vec3::new(p.x, obs.bottom, p.y),
                Vec3::new(p.x, obs.top, p.y),
                Color::srgb(1.0, 1.0, 1.0),
            );
        }
    }
}

//------------------------------TESTS---------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapAssets;

    fn assets() -> MapAssets {
        MapAssets {
            wall: Handle::default(),
            floor: Handle::default(),
            ceiling: Handle::default(),
            obstacle_side: Handle::default(),
            obstacle_top: Handle::default(),
            obstacle_bottom: Handle::default(),
        }
    }

    fn rect(map: &mut Map, x0: f32, y0: f32, x1: f32, y1: f32) -> usize {
        let assets = assets();
        map.add_sector_from_polygon(
            &[
                Vec2::new(x0, y0),
                Vec2::new(x1, y0),
                Vec2::new(x1, y1),
                Vec2::new(x0, y1),
            ],
            &assets,
        )
        .unwrap()
    }

    #[test]
    fn pick_target_hits_floor_and_ceiling() {
        let mut map = Map::default();
        rect(&mut map, 0.0, 0.0, 10.0, 10.0);
        assert_eq!(map.sectors[0].floor_height, 0.0);
        assert_eq!(map.sectors[0].ceiling_height, 20.0);

        // Straight down through the sector center hits its floor.
        let down = Ray3d::new(Vec3::new(5.0, 5.0, 5.0), Dir3::NEG_Y);
        assert_eq!(pick_target(&down, &map), Some(PickResult::Sector(0)));

        // Straight up from inside hits its ceiling.
        let up = Ray3d::new(Vec3::new(5.0, 5.0, 5.0), Dir3::Y);
        assert_eq!(pick_target(&up, &map), Some(PickResult::Sector(0)));

        // Outside the polygon: no hit.
        let outside = Ray3d::new(Vec3::new(50.0, 5.0, 50.0), Dir3::NEG_Y);
        assert_eq!(pick_target(&outside, &map), None);
    }

    #[test]
    fn pick_obstacle_hits_body() {
        let mut map = Map::default();
        let id = rect(&mut map, 0.0, 0.0, 10.0, 10.0);
        map.add_obstacle(id, 2.0, 2.0, 4.0, 4.0, &assets()).unwrap();
        assert_eq!(map.sectors[id].obstacles[0].bottom, 0.0);
        assert_eq!(map.sectors[id].obstacles[0].top, 8.0);

        // Down through the box center (mid height 4).
        let ray = Ray3d::new(Vec3::new(3.0, 5.0, 3.0), Dir3::NEG_Y);
        assert_eq!(pick_obstacle(&ray, &map), Some((0, 0)));

        // Outside the box: no hit.
        let ray = Ray3d::new(Vec3::new(9.0, 5.0, 9.0), Dir3::NEG_Y);
        assert_eq!(pick_obstacle(&ray, &map), None);
    }

    #[test]
    fn apply_obstacle_center_translates_and_shifts_heights() {
        let mut map = Map::default();
        let id = rect(&mut map, 0.0, 0.0, 10.0, 10.0);
        map.add_obstacle(id, 2.0, 2.0, 4.0, 4.0, &assets()).unwrap();

        apply_obstacle_center(&mut map, 0, 0, Vec3::new(10.0, 3.0, 10.0));
        let (_, obs) = obstacle_ref(&map, 0, 0).unwrap();
        let c = obstacle_center(obs, &map.vertices);
        assert!((c - Vec2::new(10.0, 10.0)).length() < 1e-3);
        assert!((obs.bottom - 3.0).abs() < 1e-3);
        assert!((obs.top - 11.0).abs() < 1e-3);
    }

    #[test]
    fn obstacle_body_drag_is_vertical_only() {
        // The 3D body drag re-applies the SAME XZ with a new Y, so the
        // obstacle must never translate horizontally, only shift bottom/top.
        let mut map = Map::default();
        let id = rect(&mut map, 0.0, 0.0, 10.0, 10.0);
        map.add_obstacle(id, 2.0, 2.0, 4.0, 4.0, &assets()).unwrap();
        let before = {
            let (_, obs) = obstacle_ref(&map, 0, 0).unwrap();
            (obstacle_center(obs, &map.vertices), obs.bottom, obs.top)
        };

        let center = before.0;
        apply_obstacle_center(&mut map, 0, 0, Vec3::new(center.x, 11.0, center.y));

        let (_, obs) = obstacle_ref(&map, 0, 0).unwrap();
        let c = obstacle_center(obs, &map.vertices);
        assert!((c - before.0).length() < 1e-4, "XZ must not move");
        assert!((obs.bottom - 11.0).abs() < 1e-3);
        assert!((obs.top - 19.0).abs() < 1e-3);
    }
}
