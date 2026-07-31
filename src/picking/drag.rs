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
    /// Entity currently being dragged
    pub entity: Option<Entity>,
    /// Screen position where mouse was pressed.
    pub press_screen_pos: Vec2,
    /// 3D position where mouse was pressed.
    pub press_world_pos: Vec3,
    /// Are we past the drag threshold?
    pub is_dragging: bool,
}

const DRAG_THRESHOLD: f32 = 5.0;

pub fn on_press(
    trigger: On<Pointer<Press>>,
    mut drag_state: ResMut<DragState>,
    state: Res<crate::picking::state::PickingState>
) {
    if trigger.button != PointerButton::Primary {
        return;
    }

    drag_state.entity = Some(trigger.event_target());
    drag_state.press_screen_pos = state.cursor_pos;
    drag_state.is_dragging = false;
}

pub fn update_drag(
    mut commands: Commands,
    mut drag_state: ResMut<DragState>,
    state: Res<super::state::PickingState>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut transforms: Query<&mut Transform>,
    dragged_query: Query<&BeingDragged>,
    keyboard: Res<ButtonInput<KeyCode>>
) {
    let Some(entity) = drag_state.entity else {
        return;
    };

    // ── Mouse released: commit or cancel ─────────────────────────
    if !mouse_buttons.pressed(MouseButton::Left) {
        if drag_state.is_dragging {
            // Final snap on release (unless Alt held)
            if let Ok(mut transform) = transforms.get_mut(entity) {
                if !state.alt_held {
                    transform.translation.x = (transform.translation.x / 1.0).round() * 1.0;
                    transform.translation.z = (transform.translation.z / 1.0).round() * 1.0;
                }
            }
            commands.entity(entity).remove::<BeingDragged>();
        }
        *drag_state = DragState::default();
        return;
    }

    // ── Check drag threshold ─────────────────────────────────────
    if !drag_state.is_dragging {
        let delta = state.cursor_pos - drag_state.press_screen_pos;
        if delta.length() < DRAG_THRESHOLD {
            return; // still a potential click, don't drag yet
        }

        // Start dragging!
        drag_state.is_dragging = true;

        let Ok(transform) = transforms.get(entity) else {
            return;
        };
        let grab_point = state.mesh_hit_point.unwrap_or(transform.translation);
        let grab_offset = transform.translation - grab_point;

        commands.entity(entity).insert(BeingDragged {
            grab_offset,
            start_position: transform.translation,
        });
    }

    // ── Update dragged position ──────────────────────────────────
    let Ok(drag) = dragged_query.get(entity) else {
        return;
    };
    let Ok(mut transform) = transforms.get_mut(entity) else {
        return;
    };

    let target = if let Some(ground) = state.ground_hit_snapped {
        // Drag on ground plane: snap XZ, preserve original Y
        Vec3::new(ground.x, drag.start_position.y, ground.z) + drag.grab_offset
    } else if let Some(mesh_point) = state.mesh_hit_point {
        // Drag over mesh surface
        mesh_point + drag.grab_offset
    } else {
        return; // nowhere to drag to
    };

    transform.translation = target;
}
