use bevy::prelude::*;

/// Attached to an entity while it's being dragged.
#[derive(Component)]
pub struct BeingDragged {
    /// Offset from entity center to where user grabbed it.
    /// If you grab the corner, the corner follows the cursor.
    pub grab_offset: Vec3,

    /// Entity's position when drag started.
    /// Used for cancel (Escape) or undo.
    pub start_position: Vec3,
}

/// Resource tracking global drag state.
#[derive(Resource, Default)]
pub struct DragState {
    /// Entity we're currently dragging (if any).
    pub entity: Option<Entity>,
    /// Screen position where mouse was pressed.
    pub press_screen_pos: Vec2,
    /// 3D position where mouse was pressed.
    pub press_world_pos: Vec3,
    /// Are we past the drag threshold?
    pub is_dragging: bool,
}

pub fn on_press(
    trigger: On<Pointer<Press>>,
    mut drag_state: ResMut<DragState>,
    transforms: Query<&Transform>,
    state: Res<crate::picking::state::PickingState>
) {
    if trigger.button != PointerButton::Primary {
        return;
    }

    let entity = trigger.event_target();

    // Only start drag if we hit a selected entity, or if nothing is selected yet
    // (click-to-select-then-drag behavior)

    let Ok(transform) = transforms.get(entity) else {
        return;
    };

    // Where did we grab? Use mesh hit point if available, else entity center.
    let grab_point = state.mesh_hit_point.unwrap_or(transform.translation);
    let grab_offset = transform.translation - grab_point;

    drag_state.entity = Some(entity);
    drag_state.press_screen_pos = state.cursor_pos; // you'd add this to PickingState
    drag_state.press_world_pos = grab_point;
    drag_state.is_dragging = false;

    // Don't add BeingDragged yet — wait for threshold
}
