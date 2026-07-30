use bevy::prelude::*;

/// Single source of truth for all mouse-to-3D picking data.
/// Updated once per frame in PreUpdate.
#[derive(Resource, Default, Debug)]
pub struct PickingState {
    /// Ray from camera through mouse cursor. None if off-window.
    pub camera_ray: Option<Ray3d>,

    /// Where ray hits y=0 ground plane.
    pub ground_hit: Option<Vec3>,
    /// Ground hit snapped to grid.
    pub ground_hit_snapped: Option<Vec3>,

    /// Entity under cursor from MeshPickingPlugin.
    pub hovered_entity: Option<Entity>,
    /// Exact hit point on mesh surface.
    pub mesh_hit_point: Option<Vec3>,
    /// Surface normal at hit point.
    pub mesh_hit_normal: Option<Vec3>,

    /// Mouse button states.
    pub just_pressed: bool,
    pub just_released: bool,
    pub is_pressed: bool,

    /// Cursor position in screen pixels.
    pub cursor_pos: Vec2,
    pub cursor_pos_prev: Vec2,

    /// Modifier keys.
    pub shift_held: bool,
    pub ctrl_held: bool,
    pub alt_held: bool,
}

/// Snap position to grid on XZ plane, preserve Y.
pub fn snap_to_grid(pos: Vec3, cell_size: f32) -> Vec3 {
    Vec3::new(
        (pos.x / cell_size).round() * cell_size,
        pos.y,
        (pos.z / cell_size).round() * cell_size
    )
}
