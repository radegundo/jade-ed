use bevy::prelude::*;

/// Attached to entity while being dragged.
#[derive(Component)]
pub struct BeingDragged {
    pub grab_offset: Vec3,
    pub start_position: Vec3,
}

/// Global drag tracking.
#[derive(Resource, Default)]
pub struct DragState {
    pub entity: Option<Entity>,
    pub press_screen_pos: Vec2,
    pub is_dragging: bool,
}

const DRAG_THRESHOLD: f32 = 5.0;

/// Observer: mouse pressed on entity. Start tracking, but DON'T drag yet.
pub fn on_press(
    trigger: On<Pointer<Press>>,
    mut drag_state: ResMut<DragState>,
    state: Res<crate::picking::state::PickingState>,
    meshes: Query<(), (With<Transform>, With<Mesh3d>)>
) {
    if trigger.button != PointerButton::Primary {
        return;
    }

    // Only track presses on actual mesh entities
    if meshes.get(trigger.event_target()).is_err() {
        return;
    }

    // Don't overwrite active drag
    if drag_state.entity.is_some() {
        return;
    }

    drag_state.entity = Some(trigger.event_target());
    drag_state.press_screen_pos = state.cursor_pos;
    drag_state.is_dragging = false;
}

/// System: handle drag logic every frame.
pub fn update_drag(
    mut commands: Commands,
    mut drag_state: ResMut<DragState>,
    state: Res<crate::picking::state::PickingState>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut transforms: Query<&mut Transform>,
    dragged_query: Query<&BeingDragged>,
    keyboard: Res<ButtonInput<KeyCode>>
) {
    let Some(entity) = drag_state.entity else {
        return;
    };

    // ── Mouse released ───────────────────────────────────────────
    if !mouse_buttons.pressed(MouseButton::Left) {
        if drag_state.is_dragging {
            // Final snap
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

    // ── Check threshold, start drag if past ──────────────────────
    if !drag_state.is_dragging {
        let delta = state.cursor_pos - drag_state.press_screen_pos;
        if delta.length() < DRAG_THRESHOLD {
            return; // still a click, not a drag
        }

        // START DRAG — compute grab offset and insert component
        drag_state.is_dragging = true;

        // Use block scope to avoid double borrow
        let (grab_offset, start_pos) = {
            let Ok(transform) = transforms.get(entity) else {
                return;
            };
            let grab_point = state.mesh_hit_point.unwrap_or(transform.translation);
            let offset = transform.translation - grab_point;
            (offset, transform.translation)
        };

        commands.entity(entity).insert(BeingDragged {
            grab_offset,
            start_position: start_pos,
        });
    }

    // ── Move entity ──────────────────────────────────────────────
    let Ok(drag) = dragged_query.get(entity) else {
        return;
    };
    let Ok(mut transform) = transforms.get_mut(entity) else {
        return;
    };

    let target = if let Some(ground) = state.ground_hit_snapped {
        Vec3::new(ground.x, drag.start_position.y, ground.z) + drag.grab_offset
    } else if let Some(mesh_point) = state.mesh_hit_point {
        mesh_point + drag.grab_offset
    } else {
        return;
    };

    transform.translation = target;
}
